//! X11: un aparato del PCIe pasado a una máquina virtual, con un invitado mínimo, para la prueba (`make -C kernel
//! prueba-x86`, con el aparato `edu` de QEMU). El programa hace de monitor: asigna el aparato a la máquina, comprueba
//! lo que filtra el kernel de su configuración, emula para el invitado el acceso a la configuración por los puertos
//! 0xCF8 y 0xCFC (su BAR 0, que proyecta donde la pone el invitado, y su MSI, que emula), y le entrega la interrupción
//! del aparato. El invitado, en modo protegido de 32 bits:
//!
//! 1. lee el ID del aparato por la configuración, pone su BAR 0 en 0xC000_0000, enciende memoria y bus master y su
//!    MSI (vector 0x50);
//! 2. lee la identificación del `edu` y prueba su registro de vida, por la BAR, sin salir;
//! 3. hace que el aparato copie por DMA 64 bytes de su memoria a la del aparato y de vuelta a otro sitio de la suya;
//! 4. pide una interrupción al aparato y espera en un HLT: el monitor recibe la del anfitrión y le entrega la 0x50;
//! 5. hace que el aparato escriba por DMA en 0x0800_0000, fuera de su memoria: la IOMMU lo bloquea (lo mira el kernel).
//!
//! El remapeo de las interrupciones (que el aparato no pueda mandar otra MSI que la suya) lo prueba el kernel después,
//! cambiando a mano su MSI (con AMD-Vi el dato; con VT-d la dirección, a la entrada de otro aparato): en QEMU no vale
//! hacerlo con un DMA del aparato a 0xFEE0_0000, porque ese DMA llega a la IOMMU sin número de aparato (la AMD-Vi de
//! QEMU lo toma como del IOAPIC, y la VT-d no comprueba el origen).
//!
//! Después para la máquina, y el aparato se puede asignar a otra. Sale con 10 si todo cuadra, con 12 si no hay IOMMU y
//! la llamada se niega como debe, y con 1 si algo falla (y dice qué).
//!
//! Llaves: 1, el permiso de máquinas virtuales; 2, el aparato (`edu`).

#![no_std]
#![no_main]

dashos::programa!(principal);

#[cfg(not(target_arch = "x86_64"))]
fn principal() -> u64 {
    dashos::println!("[maquina_pci] solo en x86");
    0
}

#[cfg(target_arch = "x86_64")]
fn principal() -> u64 {
    x86::principal()
}

#[cfg(target_arch = "x86_64")]
mod x86 {
    use dashos::maquina::{self, Informe, Interrupciones};
    use dashos::println;

    core::arch::global_asm!(
        r#"
        .pushsection .rodata.invitado_pci, "a"
        .balign 16
        .global invitado_pci_inicio
        .global invitado_pci_fin
        .set BASE, 0x8000
        .set BAR, 0xc0000000
    invitado_pci_inicio:
        .code32
        movl $0x20000, %esp
        lgdtl gdtr - invitado_pci_inicio + BASE
        ljmpl $0x10, $(recarga - invitado_pci_inicio + BASE)
    recarga:
        movw $0x18, %ax
        movw %ax, %ds
        movw %ax, %es
        movw %ax, %ss
        # 1. La configuración: el ID, la BAR 0, el mando y la MSI.
        xorl %eax, %eax
        call cfgl
        movw $0x81, %dx
        outl %eax, %dx
        movl $0x10, %eax
        movl $BAR, %ebx
        call cfge
        movl $0x04, %eax
        movl $0x6, %ebx
        call cfge
        movl $0x34, %eax
        call cfgl
        andl $0xfc, %eax
    busca:
        movl %eax, %esi
        call cfgl
        cmpb $5, %al
        je msi
        movzbl %ah, %eax
        andl $0xfc, %eax
        jnz busca
        movb $0xee, %al
        outb %al, $0x86
        hlt
    msi:
        movl %eax, %edi
        leal 4(%esi), %eax
        movl $0xfee00000, %ebx
        call cfge
        leal 8(%esi), %eax
        testl $0x800000, %edi
        jz 1f
        xorl %ebx, %ebx
        call cfge
        leal 12(%esi), %eax
    1:  movl $0x50, %ebx
        call cfge
        movl %esi, %eax
        movl %edi, %ebx
        orl $0x10000, %ebx
        call cfge
        # 2. La BAR, sin salir: la identificación y el registro de vida.
        movl BAR, %eax
        movw $0x81, %dx
        outl %eax, %dx
        movl $0x12345678, BAR + 4
        movl BAR + 4, %eax
        outl %eax, %dx
        # 3. DMA: 64 bytes de 0x10000 al aparato, y del aparato a 0x11000.
        movl $0x10000, BAR + 0x80
        movl $0x40000, BAR + 0x88
        movl $64, BAR + 0x90
        movl $1, BAR + 0x98
        call espera
        movl $0x40000, BAR + 0x80
        movl $0x11000, BAR + 0x88
        movl $64, BAR + 0x90
        movl $3, BAR + 0x98
        call espera
        movl 0x11000, %eax
        movw $0x81, %dx
        outl %eax, %dx
        # 4. La interrupción: el vector 0x50 en la IDT, y el aparato la levanta.
        movl $(m50 - invitado_pci_inicio + BASE), %eax
        movw %ax, 0x1000 + 0x50 * 8
        movw $0x10, 0x1000 + 0x50 * 8 + 2
        movw $0x8e00, 0x1000 + 0x50 * 8 + 4
        shrl $16, %eax
        movw %ax, 0x1000 + 0x50 * 8 + 6
        lidtl idtr - invitado_pci_inicio + BASE
        sti
        movl $1, BAR + 0x60
        hlt
        cli
        # 5. DMA fuera de su memoria.
        movl $0x40000, BAR + 0x80
        movl $0x08000000, BAR + 0x88
        movl $64, BAR + 0x90
        movl $3, BAR + 0x98
        call espera
        movb $2, %al
        outb %al, $0x86
        movb $0, %al
        outb %al, $0xf4
        hlt
    espera:
        testl $1, BAR + 0x98
        jnz espera
        ret
    # Leer la configuración: el registro en eax; el valor, en eax.
    cfgl:
        pushl %edx
        orl $0x80000000, %eax
        movw $0xcf8, %dx
        outl %eax, %dx
        movw $0xcfc, %dx
        inl %dx, %eax
        popl %edx
        ret
    # Escribir la configuración: el registro en eax, el valor en ebx.
    cfge:
        pushl %edx
        orl $0x80000000, %eax
        movw $0xcf8, %dx
        outl %eax, %dx
        movw $0xcfc, %dx
        movl %ebx, %eax
        outl %eax, %dx
        popl %edx
        ret
    m50:
        pushl %eax
        pushl %edx
        movl BAR + 0x24, %eax
        movl %eax, BAR + 0x64
        movw $0x83, %dx
        outl %eax, %dx
        popl %edx
        popl %eax
        iretl
        .balign 8
    gdt:
        .quad 0, 0, 0x00cf9a000000ffff, 0x00cf92000000ffff
    gdtr:
        .word 31
        .long gdt - invitado_pci_inicio + BASE
    idtr:
        .word 0x7ff
        .long 0x1000
    invitado_pci_fin:
        .code64
        .popsection
        "#,
        options(att_syntax)
    );

    unsafe extern "C" {
        static invitado_pci_inicio: u8;
        static invitado_pci_fin: u8;
    }

    const PERMISO: u32 = 1;
    const APARATO: u32 = 2;
    /// Donde se proyecta aquí la RAM del invitado (2 MB, desde su física 0).
    const VA: usize = 0x30_0000_0000;
    const RAM: usize = 2 << 20;
    /// Donde pone el invitado la BAR 0.
    const BAR_INVITADO: u64 = 0xc000_0000;
    const VECTOR: u64 = 0x50;
    /// Lo que copia el aparato por DMA.
    const MUESTRA: &[u8; 64] = b"DashOS X11: un aparato pasado a una maquina copia esto por DMA!!";

    /// La configuración que ve el invitado por 0xCF8 y 0xCFC: la del aparato (por el kernel, filtrada), con su BAR 0 y
    /// su MSI emuladas aquí.
    struct Config {
        cf8: u32,
        bar0: u32,
        /// Dónde está la MSI del aparato y si es de 64 bits.
        msi: usize,
        msi64: bool,
        /// Lo que escribe el invitado en la MSI: control, dirección y dato.
        control: u32,
        direccion: u64,
        dato: u32,
        /// Escrituras que el kernel no dejó pasar (se ignoran, como un registro de solo lectura).
        negadas: u32,
    }

    struct Prueba {
        m: u32,
        inf: Informe,
        irq: Interrupciones,
        valor: u64,
        cfg: Config,
    }

    impl Prueba {
        fn mal(&self, que: &str, r: i64) -> bool {
            let i = &self.inf;
            println!(
                "[maquina_pci] MAL ({}): motivo {}, pc {:#x}, dirección {:#x}, valor {:#x}, tamaño {}, x1 {}",
                que, r, i.pc, i.direccion, i.valor, i.tamano, i.x1
            );
            false
        }

        /// Lee la configuración que ve el invitado: el registro `off` (alineado a `bytes`).
        fn cfg_leer(&self, off: usize, bytes: usize) -> u32 {
            let c = &self.cfg;
            // Solo el aparato 0:0.0.
            if c.cf8 & 0x00ff_f800 != 0 {
                return u32::MAX;
            }
            let dw = off & !3;
            let palabra: u32 = if dw == 0x10 {
                c.bar0
            } else if (0x14..0x28).contains(&dw) || dw == 0x30 {
                0
            } else if c.msi == 0 {
                dashos::maquina_aparato_leer(self.m, APARATO, dw as u64, 4) as u32
            } else if dw == c.msi {
                let real = dashos::maquina_aparato_leer(self.m, APARATO, dw as u64, 4) as u32;
                (real & 0xffff) | ((real >> 16) & !1) << 16 | (c.control & 1) << 16
            } else if dw == c.msi + 4 {
                c.direccion as u32
            } else if c.msi64 && dw == c.msi + 8 {
                (c.direccion >> 32) as u32
            } else if dw == c.msi + if c.msi64 { 12 } else { 8 } {
                c.dato
            } else {
                dashos::maquina_aparato_leer(self.m, APARATO, dw as u64, 4) as u32
            };
            let d = 8 * (off - dw);
            let mascara = if bytes >= 4 { u32::MAX } else { (1u32 << (8 * bytes)) - 1 };
            (palabra >> d) & mascara
        }

        /// Escribe lo que escribe el invitado en la configuración.
        fn cfg_escribir(&mut self, off: usize, bytes: usize, v: u32) {
            if self.cfg.cf8 & 0x00ff_f800 != 0 {
                return;
            }
            let dw = off & !3;
            let c = &mut self.cfg;
            if dw == 0x10 && bytes == 4 {
                // La BAR 0: el monitor la proyecta donde la pone el invitado (una vez).
                let nueva = v & !0xf;
                if v != u32::MAX && c.bar0 == 0 && nueva != 0 {
                    c.bar0 = nueva;
                    let r = dashos::maquina_aparato_bar(self.m, APARATO, 0, nueva as u64);
                    if r != 0 {
                        println!("[maquina_pci] MAL: proyectar la BAR 0 en {:#x}: {}", nueva, r);
                    }
                }
                return;
            }
            if c.msi != 0 && (c.msi..c.msi + if c.msi64 { 16 } else { 12 }).contains(&dw) && bytes == 4 {
                match dw - c.msi {
                    0 => c.control = v >> 16,
                    4 => c.direccion = (c.direccion & !0xffff_ffff) | v as u64,
                    8 if c.msi64 => c.direccion = (c.direccion & 0xffff_ffff) | (v as u64) << 32,
                    _ => c.dato = v,
                }
                return;
            }
            if dashos::maquina_aparato_escribir(self.m, APARATO, off as u64, bytes as u64, v as u64) < 0 {
                self.cfg.negadas += 1;
            }
        }

        /// La siguiente salida que no sea de la configuración (que se atiende aquí) ni del reloj del anfitrión.
        fn salida(&mut self) -> i64 {
            for _ in 0..100_000 {
                let r = dashos::maquina_correr_irq(self.m, &mut self.inf, self.valor, &mut self.irq);
                self.valor = 0;
                if r == maquina::INTERRUMPIDO {
                    continue;
                }
                let i = self.inf;
                if r == maquina::PUERTO && (i.direccion == 0xcf8 || (0xcfc..0xd00).contains(&i.direccion)) {
                    let lee = i.x1 == 1;
                    if i.direccion == 0xcf8 {
                        if lee {
                            self.valor = self.cfg.cf8 as u64;
                        } else if i.tamano == 4 {
                            self.cfg.cf8 = i.valor as u32;
                        }
                    } else if self.cfg.cf8 & (1 << 31) == 0 {
                        self.valor = if lee { u64::MAX } else { 0 };
                    } else {
                        let off = (self.cfg.cf8 & 0xfc) as usize + (i.direccion - 0xcfc) as usize;
                        if lee {
                            self.valor = self.cfg_leer(off, i.tamano as usize) as u64;
                        } else {
                            self.cfg_escribir(off, i.tamano as usize, i.valor as u32);
                        }
                    }
                    continue;
                }
                return r;
            }
            maquina::INTERRUMPIDO
        }

        /// Un OUT de 4 bytes a `puerto`: su valor.
        fn valor_de(&mut self, puerto: u64) -> Option<u64> {
            let r = self.salida();
            let i = self.inf;
            if r != maquina::PUERTO || i.direccion != puerto || i.x1 != 0 {
                self.mal("OUT con un valor", r);
                return None;
            }
            Some(i.valor)
        }
    }

    /// Lo que filtra el kernel de la configuración, visto desde el monitor.
    fn filtro(m: u32) -> bool {
        let id = dashos::maquina_aparato_leer(m, APARATO, 0, 4);
        let bar = dashos::maquina_aparato_leer(m, APARATO, 0x10, 4);
        let bar_e = dashos::maquina_aparato_escribir(m, APARATO, 0x10, 4, 0x1234_0000);
        let mando = dashos::maquina_aparato_escribir(m, APARATO, 4, 2, 0x6);
        let (tam, banderas) = dashos::maquina_aparato_info(m, APARATO, 0);
        let mut cap = (dashos::maquina_aparato_leer(m, APARATO, 0x34, 1) & 0xfc) as u64;
        let mut msi_e = 0;
        while cap != 0 {
            let p = dashos::maquina_aparato_leer(m, APARATO, cap, 4) as u64;
            if p & 0xff == 5 {
                msi_e = dashos::maquina_aparato_escribir(m, APARATO, cap + 4, 4, 0xfee0_0000);
                break;
            }
            cap = (p >> 8) & 0xfc;
        }
        let bien = id == 0x11e8_1234 && bar == 0 && bar_e == dashos::E_DERECHOS && mando == 0 && tam == 1 << 20 && banderas == 0 && msi_e == dashos::E_DERECHOS;
        if !bien {
            println!(
                "[maquina_pci] MAL: configuración: ID {:#x}, BAR 0 {:#x}, escribir la BAR {}, el mando {}, BAR 0 de {:#x} ({:#x}), escribir la MSI {}",
                id, bar, bar_e, mando, tam, banderas, msi_e
            );
        }
        bien
    }

    pub fn principal() -> u64 {
        let ram = dashos::memoria_crear(RAM);
        if dashos::memoria_proyectar(ram, VA, true) != 0 {
            println!("[maquina_pci] no se pudo proyectar la RAM del invitado");
            return 1;
        }
        // SAFETY: el invitado está en este programa, entre sus marcas; la RAM está proyectada aquí.
        unsafe {
            let ini = &raw const invitado_pci_inicio;
            let n = &raw const invitado_pci_fin as usize - ini as usize;
            core::ptr::copy_nonoverlapping(ini, (VA + 0x8000) as *mut u8, n);
            core::ptr::copy_nonoverlapping(MUESTRA.as_ptr(), (VA + 0x10000) as *mut u8, 64);
        }
        let m = dashos::maquina_crear(PERMISO, ram, 0, 0x8000, 0);
        if m < 0 {
            println!("[maquina_pci] maquina_crear: {}", m);
            return 1;
        }
        let m = m as u32;
        let (irq, banderas) = dashos::maquina_aparato(m, APARATO);
        if irq == dashos::E_NO_HAY {
            println!("[maquina_pci] sin IOMMU: pasar el aparato se niega (E_NO_HAY)");
            return 12;
        }
        if irq <= 0 {
            println!("[maquina_pci] MAL: maquina_aparato: {}", irq);
            return 1;
        }
        let irq = irq as u32;
        let remapeadas = banderas & maquina::APARATO_REMAPEADAS != 0;
        let (otra, _) = dashos::maquina_aparato(m, APARATO);
        if otra != dashos::E_LIMITE {
            println!("[maquina_pci] MAL: asignarlo dos veces da {}", otra);
            return 1;
        }
        println!("[maquina_pci] aparato asignado (interrupciones {})", if remapeadas { "remapeadas por la IOMMU" } else { "SIN remapeo" });
        if !filtro(m) {
            return 1;
        }
        println!("[maquina_pci] configuración filtrada por el kernel (BAR y MSI no se tocan): bien");
        let (c0, c1) = dashos::canal_crear();
        if dashos::interrupcion_asociar(irq, c0) != 0 {
            println!("[maquina_pci] MAL: interrupcion_asociar");
            return 1;
        }
        // Dónde está su MSI, para emularla.
        let mut cfg = Config { cf8: 0, bar0: 0, msi: 0, msi64: false, control: 0, direccion: 0, dato: 0, negadas: 0 };
        let mut cap = (dashos::maquina_aparato_leer(m, APARATO, 0x34, 1) & 0xfc) as usize;
        while cap != 0 {
            let p = dashos::maquina_aparato_leer(m, APARATO, cap as u64, 4) as u32;
            if p & 0xff == 5 {
                cfg.msi = cap;
                cfg.msi64 = p & (1 << 23) != 0;
                break;
            }
            cap = ((p >> 8) & 0xfc) as usize;
        }
        let mut p = Prueba { m, inf: Informe::default(), irq: Interrupciones::default(), valor: 0, cfg };
        if !invitado(&mut p, irq, c1) {
            return 1;
        }
        // Parar la máquina suelta el aparato: se puede pasar a otra.
        dashos::maquina_parar(m);
        let ram2 = dashos::memoria_crear(1 << 16);
        let m2 = dashos::maquina_crear(PERMISO, ram2, 0, 0, 0);
        let (irq2, _) = if m2 > 0 { dashos::maquina_aparato(m2 as u32, APARATO) } else { (m2, 0) };
        if irq2 <= 0 {
            println!("[maquina_pci] MAL: tras parar la máquina, asignarlo a otra da {}", irq2);
            return 1;
        }
        dashos::maquina_parar(m2 as u32);
        println!("[maquina_pci] al parar la máquina el aparato queda libre, y se asigna a otra: bien");
        10
    }

    fn invitado(p: &mut Prueba, irq: u32, canal: u32) -> bool {
        // 1 y 2: la configuración y la BAR.
        let Some(id) = p.valor_de(0x81) else { return false };
        let Some(ident) = p.valor_de(0x81) else { return false };
        let msi_bien = p.cfg.control & 1 != 0 && p.cfg.direccion == 0xfee0_0000 && p.cfg.dato as u64 == VECTOR;
        if id != 0x11e8_1234 || p.cfg.bar0 as u64 != BAR_INVITADO || !msi_bien {
            println!(
                "[maquina_pci] MAL: el invitado lee el ID {:#x}, pone la BAR en {:#x}, la MSI {:#x} {:#x} {:#x}",
                id, p.cfg.bar0, p.cfg.control, p.cfg.direccion, p.cfg.dato
            );
            return false;
        }
        let Some(vida) = p.valor_de(0x81) else { return false };
        if ident & 0xff != 0xed || vida != !0x1234_5678u32 as u64 {
            println!("[maquina_pci] MAL: por la BAR, identificación {:#x} y vida {:#x}", ident, vida);
            return false;
        }
        println!("[maquina_pci] el invitado lee su configuración y usa su BAR sin salir: bien");
        // 3: el DMA en su memoria.
        let Some(primero) = p.valor_de(0x81) else { return false };
        // SAFETY: la RAM del invitado está proyectada aquí; el aparato ya terminó (el invitado lo esperó).
        let copia = unsafe { core::slice::from_raw_parts((VA + 0x11000) as *const u8, 64) };
        if copia != MUESTRA || primero != u32::from_le_bytes(MUESTRA[..4].try_into().unwrap()) as u64 {
            println!("[maquina_pci] MAL: el DMA del aparato no copió la muestra ({:#x})", primero);
            return false;
        }
        println!("[maquina_pci] el aparato copia por DMA en la memoria del invitado: bien");
        // 4: la interrupción. El invitado espera en un HLT; la del anfitrión llega por el canal.
        let r = p.salida();
        if r != maquina::WFI {
            return p.mal("HLT esperando la interrupción", r);
        }
        let mut buf = [0u8; 16];
        let mut llaves = [0u32; 1];
        let (n, _, _) = dashos::canal_leer_plazo(canal, &mut buf, &mut llaves, 2_000_000);
        if n < 4 {
            println!("[maquina_pci] MAL: no llega la interrupción del aparato ({})", n);
            return false;
        }
        dashos::interrupcion_terminar(irq);
        p.irq.lr[0] = p.cfg.dato as u64 | maquina::HUECO_PENDIENTE;
        let Some(estado) = p.valor_de(0x83) else { return false };
        if estado != 1 || p.irq.lr[0] != 0 {
            println!("[maquina_pci] MAL: en la interrupción, estado {:#x}, hueco {:#x}", estado, p.irq.lr[0]);
            return false;
        }
        println!("[maquina_pci] la MSI del aparato llega al anfitrión y se entrega al invitado (vector {:#x}): bien", VECTOR);
        // 5: el DMA fuera de su memoria (el kernel mira que se bloqueó) y el final.
        let r = p.salida();
        if r != maquina::PUERTO || p.inf.direccion != 0x86 || p.inf.valor != 2 {
            return p.mal("DMA fuera", r);
        }
        let r = p.salida();
        if r != maquina::PUERTO || p.inf.direccion != 0xf4 {
            return p.mal("final", r);
        }
        if p.cfg.negadas != 0 {
            println!("[maquina_pci] MAL: el kernel negó {} escrituras del invitado en la configuración", p.cfg.negadas);
            return false;
        }
        true
    }
}
