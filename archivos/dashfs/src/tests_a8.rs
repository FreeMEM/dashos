//! DashOS, A8: el asignador calcula el hermano al liberar y busca por rango al reservar un bloque concreto, en vez de
//! recorrer el nivel entero. Tiene que hacer exactamente lo mismo que antes: se compara con el algoritmo de antes en
//! secuencias al azar de reservas, liberaciones y reservas exactas.

use alloc::collections::BTreeSet;

use crate::{Allocator, BlockAddr, BlockLevel, BlockMeta};

/// El asignador de antes, solo lo que cambió (liberar y reservar exacto), sobre los mismos niveles.
struct Antes {
    levels: Vec<BTreeSet<u64>>,
}

impl Antes {
    fn deallocate(&mut self, index0: u64, level0: usize) {
        let (mut index, mut level) = (index0, level0);
        loop {
            while level >= self.levels.len() {
                self.levels.push(BTreeSet::new());
            }
            let level_size = 1 << level;
            let next_size = level_size << 1;
            let mut found = false;
            for &level_index in self.levels[level].iter() {
                if index % next_size == 0 && index + level_size == level_index {
                    self.levels[level].remove(&level_index);
                    found = true;
                    break;
                } else if level_index % next_size == 0 && level_index + level_size == index {
                    self.levels[level].remove(&level_index);
                    index = level_index;
                    found = true;
                    break;
                }
            }
            if !found {
                self.levels[level].insert(index);
                return;
            }
            level += 1;
        }
    }

    fn allocate_exact(&mut self, exact_index: u64) -> Option<u64> {
        let mut index_opt = None;
        for level in (0..self.levels.len()).rev() {
            let level_size = 1 << level;
            if let Some(index) = index_opt.take() {
                self.levels[level].insert(index);
                self.levels[level].insert(index + level_size);
            }
            for &start in self.levels[level].iter() {
                if start <= exact_index && start + level_size > exact_index {
                    self.levels[level].remove(&start);
                    index_opt = Some(start);
                    break;
                }
            }
        }
        index_opt
    }
}

#[test]
fn el_asignador_hace_lo_mismo_que_antes() {
    let mut x = 0x2545_f491_4f6c_dd1du64;
    let mut azar = |n: u64| {
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        x % n
    };
    for _ in 0..50 {
        // Todo libre al principio: 4096 bloques de nivel 0, liberados uno a uno (se juntan).
        let mut nuevo = Allocator::default();
        let mut antes = Antes { levels: Vec::new() };
        for i in 0..4096u64 {
            nuevo.deallocate(unsafe { BlockAddr::new(i, BlockMeta::default()) });
            antes.deallocate(i, 0);
        }
        assert_eq!(nuevo.levels(), &antes.levels);
        let mut usados: Vec<(u64, usize)> = Vec::new();
        for _ in 0..3000 {
            match azar(3) {
                0 => {
                    let nivel = azar(4) as usize;
                    let meta = BlockMeta::new(BlockLevel(nivel));
                    if let Some(a) = nuevo.allocate(meta) {
                        // `allocate` no cambió: se replica en los niveles de antes quitando lo mismo.
                        antes.levels = nuevo.levels().clone();
                        usados.push((a.index(), nivel));
                    }
                }
                1 if !usados.is_empty() => {
                    let (i, nivel) = usados.swap_remove(azar(usados.len() as u64) as usize);
                    nuevo.deallocate(unsafe { BlockAddr::new(i, BlockMeta::new(BlockLevel(nivel))) });
                    antes.deallocate(i, nivel);
                }
                _ => {
                    let i = azar(4096);
                    let a = nuevo.allocate_exact(unsafe { BlockAddr::new(i, BlockMeta::default()) }).map(|a| a.index());
                    let b = antes.allocate_exact(i);
                    assert_eq!(a, b, "reservar exacto el {i}");
                    if a.is_some() {
                        usados.push((i, 0));
                    }
                }
            }
            assert_eq!(nuevo.levels(), &antes.levels);
        }
    }
}

#[test]
fn reservar_exacto_otro_nivel_no_hace_panico() {
    let mut a = Allocator::default();
    a.deallocate(unsafe { BlockAddr::new(0, BlockMeta::new(BlockLevel(3))) });
    assert_eq!(a.allocate_exact(unsafe { BlockAddr::new(0, BlockMeta::new(BlockLevel(2))) }), None);
}

/// A8: rehacer el diario del asignador (cada `ALLOC_GC_THRESHOLD` generaciones) con el disco fragmentado: lo que tarda,
/// y que al reabrir el disco el asignador sale igual. Para medir: `cargo test --release rehacer -- --nocapture`.
#[cfg(feature = "std")]
#[test]
fn rehacer_el_diario_del_asignador() {
    use crate::{DiskMemory, FileSystem, Node, Transaction, TreePtr};
    let mut fs = FileSystem::create(DiskMemory::new(256 << 20), None, 0, 0).unwrap();
    let data = vec![7u8; 4096];
    let raiz = TreePtr::<Node>::root();
    for i in 0..6000 {
        fs.tx(|tx| {
            let n = tx.create_node(raiz, &format!("f{i}"), Node::MODE_FILE | 0o644, 0, 0)?;
            tx.write_node(n.ptr(), 0, &data, 0, 0)?;
            Ok(())
        })
        .unwrap();
    }
    for i in (0..6000).step_by(2) {
        fs.tx(|tx| tx.remove_node(raiz, &format!("f{i}"), Node::MODE_FILE).map(|_| ())).unwrap();
    }
    let trozos = fs.allocator().levels().iter().map(|l| l.len()).sum::<usize>();
    let t = std::time::Instant::now();
    Transaction::new(&mut fs).commit(true).unwrap();
    let tarda = t.elapsed();
    let antes = fs.allocator().levels().clone();
    let bloque = fs.block;
    let FileSystem { disk, .. } = fs;
    let fs = FileSystem::open(disk, None, Some(bloque), false).unwrap();
    std::eprintln!("A8 rehacer: {trozos} trozos libres; rehacer el diario {tarda:?}");
    assert_eq!(fs.allocator().levels(), &antes, "al reabrir, el asignador sale igual");
}
