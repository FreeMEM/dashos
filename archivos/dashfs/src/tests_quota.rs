//! La cuota de los volúmenes (FS10 la guardaba; S32 la aplica): escribir por encima del tope da "lleno" en ese
//! volumen y no en los demás; lo que no lo hace crecer vale siempre; el tope se cambia; y la cuenta de bloques que
//! lleva cada volumen es la que cuenta el comprobador, también con instantáneas.

use syscall::error::{EINVAL, ENOENT, ENOSPC};

use crate::check::{self, Options, Problem};
use crate::repair;
use crate::tests_snapshot::{FILE, Fs, assert_clean, new_fs, noise, reopen};
use crate::{BLOCK_SIZE, Node, TreePtr};

fn write(fs: &mut Fs, volume: u64, name: &str, data: &[u8]) -> syscall::error::Result<()> {
    fs.tx_in(volume, |tx| {
        let node = match tx.find_node(TreePtr::root(), name) {
            Ok(node) => node.ptr(),
            Err(_) => tx.create_node(TreePtr::root(), name, FILE, 0, 0)?.ptr(),
        };
        tx.write_node(node, 0, data, 0, 0).map(|_| ())
    })
}

fn remove(fs: &mut Fs, volume: u64, name: &str) -> syscall::error::Result<()> {
    fs.tx_in(volume, |tx| tx.remove_node(TreePtr::root(), name, Node::MODE_FILE).map(|_| ()))
}

fn read(fs: &mut Fs, volume: u64, name: &str) -> Vec<u8> {
    fs.tx_in(volume, |tx| {
        let node = tx.find_node(TreePtr::root(), name)?;
        let mut data = vec![0; node.data().size() as usize];
        let mut done = 0;
        while done < data.len() {
            done += tx.read_node_inner(&node, done as u64, &mut data[done..])?;
        }
        Ok(data)
    })
    .unwrap()
}

fn space(fs: &mut Fs, volume: u64) -> (u64, u64) {
    fs.tx(|tx| tx.volume_space(volume)).unwrap()
}

/// Lo que cuenta el comprobador que usa el volumen.
fn counted(fs: &mut Fs, volume: u64) -> u64 {
    let report = fs.tx(|tx| check::check(tx, Options::default(), true)).unwrap();
    assert!(report.is_clean(), "{:?}", report.problems);
    report.volume_used[&volume]
}

fn errno<T: core::fmt::Debug>(r: syscall::error::Result<T>) -> i32 {
    r.unwrap_err().errno
}

#[test]
fn full_in_one_volume_only() {
    let mut fs = new_fs(64);
    let ana = fs.tx(|tx| tx.create_volume("Ana", 1, 0)).unwrap();
    let beto = fs.tx(|tx| tx.create_volume("Beto", 1, 0)).unwrap();
    // Recién hecho: los cuatro bloques de su árbol y su raíz.
    assert_eq!(space(&mut fs, ana), (5, 0));
    assert_eq!(counted(&mut fs, ana), 5);

    // Un tope de 1 MiB.
    fs.tx(|tx| tx.set_volume_quota(ana, 256)).unwrap();
    write(&mut fs, ana, "a", &noise(500_000, 1)).unwrap();
    let (used, quota) = space(&mut fs, ana);
    assert!(used > 122 && used < 140 && quota == 256, "{used}");
    // Lo que no cabe da "lleno", entero: ni medio fichero ni el nombre.
    let free = fs.allocator().free();
    assert_eq!(errno(write(&mut fs, ana, "b", &noise(600_000, 2))), ENOSPC);
    assert_eq!(space(&mut fs, ana), (used, 256));
    assert_eq!(fs.allocator().free(), free);
    assert!(fs.tx_in(ana, |tx| tx.find_node(TreePtr::root(), "b").map(|_| ())).is_err());
    assert_eq!(read(&mut fs, ana, "a"), noise(500_000, 1));
    // A los demás no les pasa nada: el otro volumen y el principal escriben mucho más que eso.
    write(&mut fs, beto, "b", &noise(4_000_000, 3)).unwrap();
    write(&mut fs, 0, "c", &noise(4_000_000, 4)).unwrap();
    assert_eq!(space(&mut fs, beto).1, 0);
    // Hasta el tope sí cabe, bloque a bloque; y ni uno más.
    let mut i = 0;
    while write(&mut fs, ana, &format!("p{i}"), &noise(4096, 10 + i)).is_ok() {
        i += 1;
        assert!(i < 300);
    }
    let (used, _) = space(&mut fs, ana);
    assert!(used > 250 && used <= 256, "{used}");
    assert_eq!(counted(&mut fs, ana), used);

    // Lleno: escribir encima de lo mismo vale (no crece), borrar vale, y lo borrado deja sitio.
    write(&mut fs, ana, "a", &noise(500_000, 5)).unwrap();
    assert_eq!(read(&mut fs, ana, "a"), noise(500_000, 5));
    assert_eq!(errno(write(&mut fs, ana, "otro", &noise(100_000, 6))), ENOSPC);
    remove(&mut fs, ana, "p0").unwrap();
    remove(&mut fs, ana, "p1").unwrap();
    remove(&mut fs, ana, "a").unwrap();
    write(&mut fs, ana, "otro", &noise(100_000, 6)).unwrap();

    // Cambiar el tope: más, y cabe lo que no cabía; menos de lo usado, y no crece pero se puede borrar; sin tope.
    assert_eq!(errno(write(&mut fs, ana, "b", &noise(900_000, 2))), ENOSPC);
    fs.tx(|tx| tx.set_volume_quota(ana, 1024)).unwrap();
    write(&mut fs, ana, "b", &noise(900_000, 2)).unwrap();
    fs.tx(|tx| tx.set_volume_quota(ana, 10)).unwrap();
    assert_eq!(errno(write(&mut fs, ana, "x", b"un fichero corto, dentro del nodo")), ENOSPC);
    remove(&mut fs, ana, "otro").unwrap();
    assert!(space(&mut fs, ana).0 > 10);
    fs.tx(|tx| tx.set_volume_quota(ana, 0)).unwrap();
    write(&mut fs, ana, "grande", &noise(3_000_000, 7)).unwrap();

    // Abierto de nuevo: la cuenta y el tope siguen ahí.
    fs.tx(|tx| tx.set_volume_quota(ana, 2000)).unwrap();
    let used = space(&mut fs, ana).0;
    let mut fs = reopen(fs);
    assert_eq!(space(&mut fs, ana), (used, 2000));
    assert_eq!(counted(&mut fs, ana), used);
    assert_eq!(counted(&mut fs, beto), space(&mut fs, beto).0);
    assert_eq!(errno(write(&mut fs, ana, "no", &noise(6_000_000, 8))), ENOSPC);
    assert_clean(&mut fs, "al final");

    // Hostil: el principal no tiene tope, ni un volumen que no existe.
    assert_eq!(errno(fs.tx(|tx| tx.set_volume_quota(0, 5))), ENOENT);
    assert_eq!(errno(fs.tx(|tx| tx.set_volume_quota(77, 5))), ENOENT);
    assert_eq!(errno(fs.tx_in(ana, |tx| tx.set_volume_quota(beto, 5))), EINVAL);
    assert_eq!(errno(fs.tx(|tx| tx.volume_space(0))), ENOENT);
}

/// Lo que solo ven las instantáneas cuenta: borrar un fichero que una instantánea guarda no deja sitio; borrar la
/// instantánea, sí. Un bloque compartido cuenta una vez.
#[test]
fn snapshots_count() {
    let mut fs = new_fs(64);
    let ana = fs.tx(|tx| tx.create_volume("Ana", 1, 0)).unwrap();
    fs.tx(|tx| tx.set_volume_quota(ana, 512)).unwrap();
    write(&mut fs, ana, "a", &noise(1_000_000, 1)).unwrap();
    let before = space(&mut fs, ana).0;
    fs.tx_in(ana, |tx| tx.create_snapshot("una", 1, 0)).unwrap();
    // La instantánea no dobla la cuenta: comparte los bloques (y gasta alguno en sus listas).
    let with_snapshot = space(&mut fs, ana).0;
    assert!(with_snapshot >= before && with_snapshot < before + 4, "{before} {with_snapshot}");
    assert_eq!(counted(&mut fs, ana), with_snapshot);
    // Borrado del volumen, la instantánea lo sigue viendo: sigue contando, y otro igual no cabe.
    remove(&mut fs, ana, "a").unwrap();
    let held = space(&mut fs, ana).0;
    assert!(held + 8 > with_snapshot, "{held}");
    assert_eq!(counted(&mut fs, ana), held);
    assert_eq!(errno(write(&mut fs, ana, "b", &noise(1_100_000, 2))), ENOSPC);
    // Escribir encima de lo que ve la instantánea sí crece: lo viejo se queda.
    write(&mut fs, ana, "c", &noise(400_000, 3)).unwrap();
    fs.tx_in(ana, |tx| tx.create_snapshot("dos", 2, 0)).unwrap();
    write(&mut fs, ana, "c", &noise(400_000, 4)).unwrap();
    // Otra vez encima: lo de después de la instantánea se suelta, así que no crece. Uno nuevo ya no cabe.
    write(&mut fs, ana, "c", &noise(400_000, 5)).unwrap();
    assert_eq!(errno(write(&mut fs, ana, "d", &noise(400_000, 6))), ENOSPC);
    assert_eq!(counted(&mut fs, ana), space(&mut fs, ana).0);
    // Sin las instantáneas, queda lo del volumen; y volver a una deja la cuenta como estaba.
    fs.tx_in(ana, |tx| tx.delete_snapshot("una")).unwrap();
    let after = space(&mut fs, ana).0;
    assert!(after + 240 < held + 200, "{after}");
    assert_eq!(counted(&mut fs, ana), after);
    write(&mut fs, ana, "b", &noise(1_100_000, 2)).unwrap();
    fs.tx_in(ana, |tx| tx.rollback_snapshot("dos")).unwrap();
    assert_eq!(counted(&mut fs, ana), space(&mut fs, ana).0);
    fs.tx_in(ana, |tx| tx.delete_snapshot("dos")).unwrap();
    assert_eq!(counted(&mut fs, ana), space(&mut fs, ana).0);
    assert_eq!(read(&mut fs, ana, "c"), noise(400_000, 3));
}

/// Un volumen de antes (sin contar) se cuenta la primera vez que cambia; una cuenta mala la ve el comprobador y la
/// arregla la reparación; y mudar bloques al encoger no la cambia.
#[test]
fn old_volumes_damage_and_shrinking() {
    let mut fs = new_fs(48);
    write(&mut fs, 0, "relleno", &noise(12_000_000, 9)).unwrap();
    let ana = fs.tx(|tx| tx.create_volume("Ana", 1, 0)).unwrap();
    write(&mut fs, ana, "a", &noise(700_000, 1)).unwrap();
    fs.tx_in(ana, |tx| tx.create_snapshot("una", 1, 0)).unwrap();
    write(&mut fs, ana, "a", &noise(300_000, 2)).unwrap();
    remove(&mut fs, 0, "relleno").unwrap();
    let used = space(&mut fs, ana).0;
    assert_eq!(counted(&mut fs, ana), used);

    // Como uno de antes: sin contar. El comprobador no se queja, y el tope no se aplica hasta que se cuente.
    let forget = |fs: &mut Fs, to: u64| {
        fs.tx(|tx| {
            let mut volume = tx.volumes()?.into_iter().find(|v| v.id() == ana).unwrap();
            volume.set_used(to);
            tx.volume_changes.insert(ana, Some(volume));
            tx.header_changed = true;
            Ok(())
        })
        .unwrap();
    };
    forget(&mut fs, 0);
    assert_eq!(space(&mut fs, ana).0, 0);
    assert_clean(&mut fs, "sin contar");
    fs.tx(|tx| tx.set_volume_quota(ana, used + 20)).unwrap();
    // El primer cambio lo cuenta, y el tope vale desde ese mismo cambio.
    assert_eq!(errno(write(&mut fs, ana, "b", &noise(200_000, 3))), ENOSPC);
    assert_eq!(space(&mut fs, ana).0, 0);
    write(&mut fs, ana, "b", &noise(20_000, 3)).unwrap();
    let now = space(&mut fs, ana).0;
    assert!(now > used && now <= used + 20, "{used} {now}");
    assert_eq!(counted(&mut fs, ana), now);

    // Una cuenta mala: el comprobador la ve y la reparación la pone bien.
    forget(&mut fs, now + 1000);
    let report = fs.tx(|tx| check::check(tx, Options::default(), true)).unwrap();
    assert_eq!(
        report.problems,
        vec![Problem::VolumeUsed { name: "Ana".into(), stored: now + 1000, counted: now }]
    );
    let (_, after) = repair::repair(&mut fs, Options::default(), true, "Perdidos").unwrap();
    assert!(after.unwrap().is_clean());
    assert_eq!(space(&mut fs, ana), (now, used + 20));

    let a = read(&mut fs, ana, "a");
    // Encoger muda sus bloques (estaban arriba) y la cuenta no cambia.
    let done = fs.shrink_min().unwrap();
    assert!(done.moved > 200, "{done:?}");
    assert_eq!(space(&mut fs, ana).0, now);
    assert_eq!(counted(&mut fs, ana), now);
    assert!(fs.header.size() < 6 * 1024 * 1024 + 512 * BLOCK_SIZE);
    assert!(read(&mut fs, ana, "a") == a);
}
