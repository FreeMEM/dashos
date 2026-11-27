//! X10: el hipervisor de x86 con un invitado mínimo, escrito para la prueba (`make -C kernel prueba-x86`). Lo que se
//! espera de cada salida, en orden: E/S de puertos (OUT de 1, 2 y 4 bytes, IN), accesos a aparatos (MOV de un
//! inmediato, MOV a registro, MOVZX, MOV a AH, de 16 bits, uno que no contesta nadie), CPUID (hipervisor, sin VMX ni
//! SVM, "DashOSDashOS"), la hipercall, MSR que van al servicio y EFER (del kernel), una interrupción entregada a un
//! HLT, el temporizador (el plazo del TSC, esperado en el kernel), el paso a modo largo (accesos de 8 bytes por una
//! dirección alta, con REX) y una segunda vCPU que empieza en modo real. Sale con 10 si todo cuadra, 11 si no hay
//! hipervisor y 1 si algo falla (y dice qué).
//!
//! Llaves: 1, el permiso de máquinas virtuales.
//!
//! En ARM no hace nada: el hipervisor de ARM se prueba con la caja Linux.

#![no_std]
#![no_main]

dashos::programa!(principal);

#[cfg(not(target_arch = "x86_64"))]
fn principal() -> u64 {
    dashos::println!("[maquina] solo en x86");
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

    // El invitado: código de 32 bits que se copia a la dirección física 0x8000 del invitado (`BASE`) y pasa a 64; y uno
    // de 16 bits para la segunda vCPU, en la 0x9000. Las direcciones absolutas, con `BASE` más el desplazamiento.
    core::arch::global_asm!(
        r#"
        .pushsection .rodata.invitado_x86, "a"
        .balign 16
        .global invitado_x86_inicio
        .global invitado_x86_fin
        .global invitado_x86_real
        .global invitado_x86_real_fin
        .set BASE, 0x8000
        .set APARATO, 0xfec00000
    invitado_x86_inicio:
        .code32
        movl $0x20000, %esp
        movl %esi, %eax
        movw $0x81, %dx
        outl %eax, %dx
        movb $0x42, %al
        outb %al, $0x80
        movw $0x1234, %dx
        movw $0xbeef, %ax
        outw %ax, %dx
        movw $0x5678, %dx
        inl %dx, %eax
        movw $0x81, %dx
        outl %eax, %dx
        movl $0x11223344, APARATO
        movl APARATO + 0x10, %ebx
        movl %ebx, %eax
        outl %eax, %dx
        movzbl APARATO + 0x20, %ecx
        movl %ecx, %eax
        outl %eax, %dx
        movw $0xa55a, %ax
        movw %ax, APARATO + 0x30
        movl $0x11111111, %eax
        movb APARATO + 0x21, %ah
        outl %eax, %dx
        movl APARATO + 0x60, %eax
        outl %eax, %dx
        movl $1, %eax
        cpuid
        movl %ecx, %eax
        movw $0x82, %dx
        outl %eax, %dx
        movl $0x80000001, %eax
        cpuid
        movl %ecx, %eax
        movw $0x82, %dx
        outl %eax, %dx
        movl $0x40000000, %eax
        cpuid
        movl %ebx, %eax
        movw $0x82, %dx
        outl %eax, %dx
        xorl %eax, %eax
        cpuid
        cmpl $0x68747541, %ebx
        movl $0x48564300, %eax
        movl $1, %ebx
        movl $2, %ecx
        movl $3, %edx
        jne 1f
        vmmcall
        jmp 2f
    1:  vmcall
    2:  movw $0x81, %dx
        outl %eax, %dx
        movl $0x4b564d00, %ecx
        movl $0x05060708, %eax
        movl $0x01020304, %edx
        wrmsr
        movl $0x12345, %ecx
        rdmsr
        movl %edx, %ebx
        movw $0x81, %dx
        outl %eax, %dx
        movl %ebx, %eax
        outl %eax, %dx
        movl $0xc0000080, %ecx
        rdmsr
        movw $0x81, %dx
        outl %eax, %dx
        lgdtl gdtr - invitado_x86_inicio + BASE
        ljmpl $0x10, $(recarga - invitado_x86_inicio + BASE)
    recarga:
        movw $0x18, %ax
        movw %ax, %ds
        movw %ax, %es
        movw %ax, %ss
        movw %ax, %fs
        movw %ax, %gs
        movl $(m40 - invitado_x86_inicio + BASE), %eax
        movw %ax, 0x1000 + 0x40 * 8
        movw $0x10, 0x1000 + 0x40 * 8 + 2
        movw $0x8e00, 0x1000 + 0x40 * 8 + 4
        shrl $16, %eax
        movw %ax, 0x1000 + 0x40 * 8 + 6
        movl $(m41 - invitado_x86_inicio + BASE), %eax
        movw %ax, 0x1000 + 0x41 * 8
        movw $0x10, 0x1000 + 0x41 * 8 + 2
        movw $0x8e00, 0x1000 + 0x41 * 8 + 4
        shrl $16, %eax
        movw %ax, 0x1000 + 0x41 * 8 + 6
        lidtl idtr - invitado_x86_inicio + BASE
        sti
        hlt
        cli
        rdtsc
        addl $1000000, %eax
        adcl $0, %edx
        movl $0x6e0, %ecx
        wrmsr
        sti
        hlt
        cli
        movl $0x5003, 0x4000
        movl $0x6003, 0x5000
        movl $0x83, 0x6000
        movl $0x7003, 0x4800
        movl $0xb003, 0x7018
        movl $0xfec00083, 0xb000 + 0x1f6 * 8
        movl $0x4000, %eax
        movl %eax, %cr3
        movl %cr4, %eax
        orl $0x20, %eax
        movl %eax, %cr4
        movl $0xc0000080, %ecx
        rdmsr
        orl $0x100, %eax
        wrmsr
        movl %cr0, %eax
        orl $0x80000000, %eax
        movl %eax, %cr0
        ljmpl $0x20, $(largo - invitado_x86_inicio + BASE)
    m40:
        pushl %eax
        movb $0x40, %al
        outb %al, $0x83
        popl %eax
        iretl
    m41:
        pushl %eax
        movb $0x41, %al
        outb %al, $0x84
        popl %eax
        iretl
        .balign 8
    gdt:
        .quad 0, 0, 0x00cf9a000000ffff, 0x00cf92000000ffff, 0x00af9a000000ffff
    gdtr:
        .word 39
        .long gdt - invitado_x86_inicio + BASE
    idtr:
        .word 0x7ff
        .long 0x1000
        .code64
    largo:
        movabsq $0xffff8000fec00040, %rax
        movq (%rax), %rbx
        movw $0x81, %dx
        movl %ebx, %eax
        outl %eax, %dx
        shrq $32, %rbx
        movl %ebx, %eax
        outl %eax, %dx
        movabsq $0xffff8000fec00048, %rdi
        movabsq $0x1122334455667788, %rax
        movq %rax, (%rdi)
        movl $0xc0000080, %ecx
        rdmsr
        movw $0x81, %dx
        outl %eax, %dx
        movq (%rdi), %r9
        movl %r9d, %eax
        outl %eax, %dx
        movb $0, %al
        outb %al, $0xf4
        hlt
    invitado_x86_fin:
    invitado_x86_real:
        .code16
        movb $0x99, %al
        outb %al, $0x85
        hlt
    invitado_x86_real_fin:
        .code64
        .popsection
        "#,
        options(att_syntax)
    );

    unsafe extern "C" {
        static invitado_x86_inicio: u8;
        static invitado_x86_fin: u8;
        static invitado_x86_real: u8;
        static invitado_x86_real_fin: u8;
    }

    const PERMISO: u32 = 1;
    /// Donde se proyecta aquí la RAM del invitado (1 MB, desde su dirección física 0).
    const VA: usize = 0x30_0000_0000;
    const RAM: usize = 1 << 20;

    struct Prueba {
        m: u32,
        inf: Informe,
        irq: Interrupciones,
        valor: u64,
        paso: u32,
    }

    impl Prueba {
        /// Una salida, saltando las del reloj del anfitrión (motivo 5).
        fn salida(&mut self) -> i64 {
            self.paso += 1;
            for _ in 0..10_000 {
                let r = dashos::maquina_correr_irq(self.m, &mut self.inf, self.valor, &mut self.irq);
                self.valor = 0;
                if r != maquina::INTERRUMPIDO {
                    return r;
                }
            }
            maquina::INTERRUMPIDO
        }

        fn mal(&self, que: &str, r: i64) -> bool {
            let i = &self.inf;
            println!(
                "[maquina] MAL en el paso {} ({}): motivo {}, pc {:#x}, dirección {:#x}, valor {:#x}, tamaño {}, código {:#x}, x1 {}",
                self.paso, que, r, i.pc, i.direccion, i.valor, i.tamano, i.esr, i.x1
            );
            false
        }

        /// Un OUT a `puerto` de `bytes` con `valor`.
        fn sale(&mut self, puerto: u64, bytes: u64, valor: u64) -> bool {
            let r = self.salida();
            let i = self.inf;
            if r != maquina::PUERTO || i.direccion != puerto || i.tamano != bytes || i.x1 != 0 || i.valor != valor {
                return self.mal("OUT", r);
            }
            true
        }

        /// Un OUT a `puerto` de 4 bytes: su valor.
        fn valor_de(&mut self, puerto: u64) -> Option<u64> {
            let r = self.salida();
            let i = self.inf;
            if r != maquina::PUERTO || i.direccion != puerto || i.tamano != 4 || i.x1 != 0 {
                self.mal("OUT con un valor", r);
                return None;
            }
            Some(i.valor)
        }

        /// Un IN de `puerto`, de `bytes`: se contesta con `respuesta`.
        fn entra(&mut self, puerto: u64, bytes: u64, respuesta: u64) -> bool {
            let r = self.salida();
            let i = self.inf;
            if r != maquina::PUERTO || i.direccion != puerto || i.tamano != bytes || i.x1 != 1 {
                return self.mal("IN", r);
            }
            self.valor = respuesta;
            true
        }

        fn escribe(&mut self, dir: u64, bytes: u64, valor: u64) -> bool {
            let r = self.salida();
            let i = self.inf;
            if r != maquina::ESCRITURA || i.direccion != dir || i.tamano != bytes || i.valor != valor {
                return self.mal("escritura en un aparato", r);
            }
            true
        }

        fn lee(&mut self, dir: u64, bytes: u64, respuesta: u64) -> bool {
            let r = self.salida();
            let i = self.inf;
            if r != maquina::LECTURA || i.direccion != dir || i.tamano != bytes {
                return self.mal("lectura de un aparato", r);
            }
            self.valor = respuesta;
            true
        }

        fn msr(&mut self, msr: u64, lee: bool, valor: u64) -> bool {
            let r = self.salida();
            let i = self.inf;
            if r != maquina::REGISTRO || i.direccion != msr || i.x1 != lee as u64 || (!lee && i.valor != valor) {
                return self.mal("MSR", r);
            }
            if lee {
                self.valor = valor;
            }
            true
        }
    }

    pub fn principal() -> u64 {
        let ram = dashos::memoria_crear(RAM);
        if dashos::memoria_proyectar(ram, VA, true) != 0 {
            println!("[maquina] no se pudo proyectar la RAM del invitado");
            return 1;
        }
        // SAFETY: los trozos del invitado están en este programa, entre sus marcas; la RAM está proyectada aquí.
        unsafe {
            let ini = &raw const invitado_x86_inicio;
            let n = &raw const invitado_x86_fin as usize - ini as usize;
            core::ptr::copy_nonoverlapping(ini, (VA + 0x8000) as *mut u8, n);
            let ini = &raw const invitado_x86_real;
            let n = &raw const invitado_x86_real_fin as usize - ini as usize;
            core::ptr::copy_nonoverlapping(ini, (VA + 0x9000) as *mut u8, n);
        }
        let m = dashos::maquina_crear(PERMISO, ram, 0, 0x8000, 0x1234_5678);
        if m == dashos::E_NO_HAY {
            println!("[maquina] sin hipervisor: maquina_crear da E_NO_HAY");
            return 11;
        }
        if m < 0 {
            println!("[maquina] maquina_crear: {}", m);
            return 1;
        }
        let mut p = Prueba { m: m as u32, inf: Informe::default(), irq: Interrupciones::default(), valor: 0, paso: 0 };
        if !puertos_y_aparatos(&mut p) || !cpuid_hipercall_msr(&mut p) || !interrupciones(&mut p) || !modo_largo(&mut p) || !otra_vcpu(&mut p) {
            return 1;
        }
        10
    }

    fn puertos_y_aparatos(p: &mut Prueba) -> bool {
        let bien = p.sale(0x81, 4, 0x1234_5678)
            && p.sale(0x80, 1, 0x42)
            && p.sale(0x1234, 2, 0xbeef)
            && p.entra(0x5678, 4, 0xcafe_f00d)
            && p.sale(0x81, 4, 0xcafe_f00d)
            && p.escribe(0xfec0_0000, 4, 0x1122_3344)
            && p.lee(0xfec0_0010, 4, 0x5566_7788)
            && p.sale(0x81, 4, 0x5566_7788)
            && p.lee(0xfec0_0020, 1, 0x1ff)
            && p.sale(0x81, 4, 0xff)
            && p.escribe(0xfec0_0030, 2, 0xa55a)
            && p.lee(0xfec0_0021, 1, 0x7e)
            && p.sale(0x81, 4, 0x1111_7e11);
        if !bien {
            return false;
        }
        // Una lectura que no contesta nadie (`maquina_aborto`): todo unos, como en un PC.
        let r = p.salida();
        if r != maquina::LECTURA || p.inf.direccion != 0xfec0_0060 {
            return p.mal("lectura sin aparato", r);
        }
        if dashos::maquina_aborto(p.m) != 0 {
            return p.mal("maquina_aborto", r);
        }
        if !p.sale(0x81, 4, 0xffff_ffff) {
            return false;
        }
        println!("[maquina] puertos (IN y OUT de 1, 2 y 4 bytes) y aparatos (MOV, MOVZX, AH, 16 bits, sin contestar): bien");
        true
    }

    fn cpuid_hipercall_msr(p: &mut Prueba) -> bool {
        let Some(c1) = p.valor_de(0x82) else { return false };
        let Some(c8) = p.valor_de(0x82) else { return false };
        let Some(firma) = p.valor_de(0x82) else { return false };
        if c1 & (1 << 31) == 0 || c1 & (1 << 5) != 0 || c8 & (1 << 2) != 0 || firma != u32::from_le_bytes(*b"Dash") as u64 {
            println!("[maquina] MAL: CPUID 1 ECX {:#x}, 0x80000001 ECX {:#x}, firma {:#x}", c1, c8, firma);
            return false;
        }
        let r = p.salida();
        let i = p.inf;
        if r != maquina::HVC || i.valor != 0x4856_4300 || i.x1 != 1 || i.x2 != 2 || i.x3 != 3 {
            return p.mal("hipercall", r);
        }
        // Lo que devuelve la hipercall, en rax.
        let mut regs = [0u64; maquina::REGISTROS];
        if dashos::maquina_registros(p.m, &mut regs) != 0 {
            return p.mal("maquina_registros", r);
        }
        regs[0] = 0x600d;
        if dashos::maquina_poner_registros(p.m, &regs) != 0 {
            return p.mal("maquina_poner_registros", r);
        }
        let bien = p.sale(0x81, 4, 0x600d)
            && p.msr(0x4b56_4d00, false, 0x0102_0304_0506_0708)
            && p.msr(0x12345, true, 0xaabb_ccdd_1122_3344)
            && p.sale(0x81, 4, 0x1122_3344)
            && p.sale(0x81, 4, 0xaabb_ccdd)
            && p.sale(0x81, 4, 0);
        if bien {
            println!("[maquina] CPUID (hipervisor, sin VMX ni SVM, DashOSDashOS), hipercall y MSR: bien");
        }
        bien
    }

    fn interrupciones(p: &mut Prueba) -> bool {
        // STI y HLT sin nada pendiente: el servicio ve el HLT y le da el vector 0x40.
        let r = p.salida();
        if r != maquina::WFI {
            return p.mal("HLT", r);
        }
        p.irq.lr[0] = 0x40 | maquina::HUECO_PENDIENTE;
        if !p.sale(0x83, 1, 0x40) {
            return false;
        }
        if p.irq.lr[0] != 0 || p.irq.lrs != maquina::LRS as u64 {
            println!("[maquina] MAL: el hueco de la interrupción no se vació ({:#x}, {} huecos)", p.irq.lr[0], p.irq.lrs);
            return false;
        }
        // El temporizador: el invitado pone el plazo del TSC y espera en un HLT; el kernel espera por él y le entrega
        // el vector 0x41.
        p.irq.temporizador = 1 | 0x41 << 8 | maquina::ESPERAR_WFI;
        let mut r = p.salida();
        let mut vueltas = 0;
        while r == maquina::WFI && vueltas < 50 {
            r = p.salida();
            vueltas += 1;
        }
        let i = p.inf;
        if r != maquina::PUERTO || i.direccion != 0x84 || i.valor != 0x41 {
            return p.mal("temporizador", r);
        }
        if p.irq.temporizador & maquina::TEMPORIZADOR_ENTREGADO == 0 {
            println!("[maquina] MAL: el temporizador no dice que se entregó");
            return false;
        }
        p.irq.temporizador = 0;
        println!("[maquina] interrupción entregada a un HLT y temporizador del invitado (plazo del TSC): bien");
        true
    }

    fn modo_largo(p: &mut Prueba) -> bool {
        let bien = p.lee(0xfec0_0040, 8, 0x0123_4567_89ab_cdef)
            && p.sale(0x81, 4, 0x89ab_cdef)
            && p.sale(0x81, 4, 0x0123_4567)
            && p.escribe(0xfec0_0048, 8, 0x1122_3344_5566_7788)
            && p.sale(0x81, 4, 0x500)
            && p.lee(0xfec0_0048, 8, 0xcafe)
            && p.sale(0x81, 4, 0xcafe);
        if !bien {
            return false;
        }
        let mut regs = [0u64; maquina::REGISTROS];
        if dashos::maquina_registros(p.m, &mut regs) != 0 || regs[9] != 0xcafe || regs[22] & 0x500 != 0x500 || regs[18] & 0x8000_0001 != 0x8000_0001 {
            println!("[maquina] MAL: registros en modo largo: r9 {:#x}, EFER {:#x}, CR0 {:#x}", regs[9], regs[22], regs[18]);
            return false;
        }
        if !p.sale(0xf4, 1, 0) {
            return false;
        }
        println!("[maquina] modo largo (tablas del invitado, dirección alta, 8 bytes, REX): bien");
        true
    }

    fn otra_vcpu(p: &mut Prueba) -> bool {
        let l = dashos::maquina_vcpu(p.m, 1, 0x9000, 0);
        if l < 0 || dashos::maquina_reiniciar_modo(l as u32, 0x9000, 0, maquina::MODO_REAL) != 0 {
            println!("[maquina] MAL: la segunda vCPU: {}", l);
            return false;
        }
        let principal = p.m;
        p.m = l as u32;
        let bien = p.sale(0x85, 1, 0x99);
        let r = p.salida();
        p.m = principal;
        if !bien {
            return false;
        }
        if r != maquina::WFI {
            return p.mal("HLT de la segunda vCPU", r);
        }
        dashos::maquina_parar(p.m);
        let r = dashos::maquina_correr(p.m, &mut p.inf, 0);
        if r != dashos::E_CERRADO {
            println!("[maquina] MAL: tras parar, correr da {}", r);
            return false;
        }
        println!("[maquina] segunda vCPU en modo real (como tras una SIPI) y parar la máquina: bien");
        true
    }
}
