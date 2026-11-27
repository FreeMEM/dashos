//! El reloj de la placa de un PC (A14 en x86): el RTC del CMOS, compatible con el MC146818, por los puertos 0x70
//! (índice) y 0x71 (dato). Se lee una vez al arrancar, como el PL031 en ARM (`crate::reloj::iniciar_hora`).
//!
//! Escrito con la hoja de datos pública del MC146818 y la descripción del RTC de los chipsets de Intel (registros 0 a
//! 9 con la hora y la fecha, A con "actualizando" en el bit 7, B con el formato: bit 2 binario, bit 1 de 24 horas).
//! La hora del RTC se toma como UTC (lo que da QEMU por omisión; un PC con Windows la tiene en hora local).

fn salida(puerto: u16, v: u8) {
    // SAFETY: los puertos del RTC (0x70 y 0x71) solo los toca este fichero, una vez al arrancar, en un solo núcleo.
    unsafe { core::arch::asm!("out dx, al", in("dx") puerto, in("al") v, options(nomem, nostack, preserves_flags)) };
}

fn entrada(puerto: u16) -> u8 {
    let v: u8;
    // SAFETY: como en `salida`.
    unsafe { core::arch::asm!("in al, dx", out("al") v, in("dx") puerto, options(nomem, nostack, preserves_flags)) };
    v
}

/// Un registro del RTC, con el NMI sin tocar (el bit 7 del índice a 0, como lo deja el firmware).
fn registro(r: u8) -> u8 {
    salida(0x70, r & 0x7f);
    entrada(0x71)
}

/// (segundos, minutos, horas, día, mes, año) tal como están en el RTC, esperando a que no esté a mitad de actualizar.
fn crudo() -> [u8; 6] {
    let mut n = 0;
    while registro(0x0a) & 0x80 != 0 && n < 1_000_000 {
        n += 1;
        core::hint::spin_loop();
    }
    [registro(0), registro(2), registro(4), registro(7), registro(8), registro(9)]
}

/// La hora del RTC en segundos desde 1970 (UTC), o `None` si lo que da no es una fecha. Se lee hasta que dos lecturas
/// seguidas coinciden: así no se mezcla un segundo con el siguiente.
pub fn segundos() -> Option<u64> {
    let mut a = crudo();
    let mut b = crudo();
    let mut vueltas = 0;
    while a != b && vueltas < 5 {
        a = b;
        b = crudo();
        vueltas += 1;
    }
    if a != b {
        return None;
    }
    let formato = registro(0x0b);
    let bcd = |v: u8| if formato & 0x04 != 0 { Some(v) } else if v & 0x0f < 10 && v >> 4 < 10 { Some((v >> 4) * 10 + (v & 0x0f)) } else { None };
    let (s, m) = (bcd(a[0])?, bcd(a[1])?);
    // En 12 horas, el bit 7 de las horas es "p. m." (12 a. m. es 0 y 12 p. m., 12).
    let pm = formato & 0x02 == 0 && a[2] & 0x80 != 0;
    let mut h = bcd(a[2] & 0x7f)?;
    if formato & 0x02 == 0 {
        h = h % 12 + if pm { 12 } else { 0 };
    }
    let (dia, mes, anyo) = (bcd(a[3])? as u64, bcd(a[4])? as u64, 2000 + bcd(a[5])? as u64);
    if s > 59 || m > 59 || h > 23 || !(1..=12).contains(&mes) || !(1..=31).contains(&dia) {
        return None;
    }
    // Días desde 1970-01-01 del calendario gregoriano (el año empieza en marzo para dejar el 29 de febrero al final).
    let (y, mm) = if mes <= 2 { (anyo - 1, mes + 9) } else { (anyo, mes - 3) };
    let era = y / 400;
    let del_anyo = (153 * mm + 2) / 5 + dia - 1;
    let de_la_era = (y - era * 400) * 365 + (y - era * 400) / 4 - (y - era * 400) / 100 + del_anyo;
    let dias = era * 146_097 + de_la_era - 719_468;
    Some(dias * 86_400 + h as u64 * 3600 + m as u64 * 60 + s as u64)
}
