#!/usr/bin/env python3
"""U7: genera t3.s, miles de instrucciones al azar (con semilla fija) que leen y escriben memoria por todos los modos
de dirección: aritmética y lógica, inmediatos, rápidas, MOVE, BCD, X, CMPM, desplazamientos de memoria, bits, Scc,
TAS, MUL y DIV (sin dividir por cero), MOVEM, MOVEP, CAS, CMP2, campos de bits estáticos y dinámicos, PACK y UNPK.
Al final guarda todos los registros. Lo que vale es que el JIT big-endian y el little-endian dejen lo mismo.

    python3 gen_t3.py > t3.s
"""
import random

R = random.Random(68040)
N = 3000
BUF = 2048          # bytes de la zona de datos que se tocan (a5 apunta al principio)


def ea(size, write=True, allow_pc=False):
    """Un modo de dirección de memoria y las instrucciones que lo preparan"""
    align = 1 if size == "b" else 2
    k = R.randrange(7)
    off = R.randrange(0, BUF - 16) & ~(align - 1)
    if k == 0:
        return [], f"{off}(a5)"
    if k == 1:
        return [f"lea {off}(a5),a4"], "(a4)"
    if k == 2:
        return [f"lea {off}(a5),a4"], "(a4)+"
    if k == 3:
        return [f"lea {off + 8}(a5),a4"], "-(a4)"
    if k == 4:
        sc = R.choice([1, 2, 4, 8])
        i = R.randrange(0, 8)
        return [f"moveq #{i},d7"], f"{off & ~7}(a5,d7.l*{sc})"
    if k == 5:
        return [f"lea {off}(a5),a3", f"move.l a3,{BUF + 64}(a5)"], f"([{BUF + 64},a5],{R.randrange(0, 8) * align})"
    return [], f"datos+{off}"


def dreg():
    return f"d{R.randrange(0, 7)}"


def imm(size):
    return R.randrange(-128, 128) if size == "b" else R.randrange(-32768, 32768) if size == "w" else R.randrange(-2**31, 2**31)


def instr():
    t = R.randrange(26)
    s = R.choice("bwl")
    if t == 0:
        p, e = ea(s)
        return p + [f"{R.choice(['add', 'sub', 'and', 'or'])}.{s} {dreg()},{e}"]
    if t == 1:
        p, e = ea(s)
        return p + [f"{R.choice(['add', 'sub', 'and', 'or', 'cmp'])}.{s} {e},{dreg()}"]
    if t == 2:
        p, e = ea(s)
        return p + [f"eor.{s} {dreg()},{e}"]
    if t == 3:
        p, e = ea(s)
        return p + [f"{R.choice(['addi', 'subi', 'andi', 'ori', 'eori', 'cmpi'])}.{s} #{imm(s)},{e}"]
    if t == 4:
        p, e = ea(s)
        return p + [f"{R.choice(['addq', 'subq'])}.{s} #{R.randrange(1, 9)},{e}"]
    if t == 5:
        p, e = ea(s)
        return p + [f"{R.choice(['neg', 'negx', 'not', 'clr', 'tst'])}.{s} {e}"]
    if t == 6:
        p1, e1 = ea(s)
        p2, e2 = ea(s)
        if "a4" in e1 and "a4" in e2:
            return p1 + [f"move.{s} {e1},{dreg()}"]
        return p1 + p2 + [f"move.{s} {e1},{e2}"]
    if t == 7:
        p, e = ea(s)
        return p + [f"move.{s} {e},{dreg()}", f"move.{s} {dreg()},{BUF + R.randrange(0, 32) * 4}(a5)"]
    if t == 8:
        o = R.randrange(16, BUF - 16) & ~3
        n = {"b": 1, "w": 2, "l": 4}[s]
        return [f"lea {o}(a5),a3", f"lea {o + 8}(a5),a4", f"{R.choice(['addx', 'subx'])}.{s} -(a3),-(a4)",
                f"{R.choice(['addx', 'subx'])}.{s} -(a3),-(a4)"] if n else []
    if t == 9:
        o = R.randrange(16, BUF - 16)
        return [f"lea {o}(a5),a3", f"lea {o + 5}(a5),a4", f"{R.choice(['abcd', 'sbcd'])} -(a3),-(a4)", f"nbcd {o}(a5)"]
    if t == 10:
        o = R.randrange(0, BUF - 16) & ~3
        return [f"lea {o}(a5),a3", f"lea {o + 4}(a5),a4", f"cmpm.{s} (a3)+,(a4)+", f"s{R.choice(['eq', 'ne', 'lt', 'hi'])} {o + 12}(a5)"]
    if t == 11:
        p, e = ea("w")
        return p + [f"{R.choice(['asl', 'asr', 'lsl', 'lsr', 'rol', 'ror', 'roxl', 'roxr'])}.w {e}"]
    if t == 12:
        p, e = ea("b")
        b = f"#{R.randrange(0, 8)}" if R.random() < 0.5 else dreg()
        return p + [f"{R.choice(['btst', 'bchg', 'bclr', 'bset'])} {b},{e}"]
    if t == 13:
        p, e = ea("b")
        return p + [f"s{R.choice(['t', 'f', 'hi', 'ls', 'cc', 'cs', 'ne', 'eq', 'vc', 'vs', 'pl', 'mi', 'ge', 'lt', 'gt', 'le'])} {e}"]
    if t == 14:
        p, e = ea("b")
        return p + [f"tas {e}"]
    if t == 15:
        p, e = ea("w")
        return p + [f"{R.choice(['muls', 'mulu'])}.w {e},{dreg()}"]
    if t == 16:
        o = R.randrange(0, BUF - 16) & ~3
        d = dreg()
        return [f"move.w #{R.randrange(1, 300)},{o}(a5)", f"{R.choice(['divs', 'divu'])}.w {o}(a5),{d}",
                f"move.l #{R.randrange(1, 70000)},{o + 4}(a5)", f"{R.choice(['divs', 'divu'])}.l {o + 4}(a5),{d}"]
    if t == 17:
        o = R.randrange(0, BUF - 16) & ~3
        return [f"{R.choice(['muls', 'mulu'])}.l {o}(a5),d6:d{R.randrange(0, 6)}"]
    if t == 18:
        o = R.randrange(0, BUF - 80) & ~1
        regs = R.choice(["d0-d3", "d1/d4/a0", "d0-d6", "a0-a2/d5", "d2"])
        w = R.choice("wl")
        if R.random() < 0.5:
            return [f"movem.{w} {regs},{o}(a5)"]
        return [f"lea {o + 64}(a5),a4", f"movem.{w} {regs},-(a4)", f"lea {o}(a5),a4",
                f"movem.{w} (a4)+,{R.choice(['d0-d1', 'd3/d5', 'a0-a1'])}", "lea 0(a5),a0", "lea 64(a5),a1", "lea 128(a5),a2"]
    if t == 19:
        o = R.randrange(0, BUF - 32)
        return [f"lea {o}(a5),a3", f"movep.{R.choice('wl')} {dreg()},1(a3)", f"movep.{R.choice('wl')} 3(a3),{dreg()}"]
    if t == 20:
        o = R.randrange(0, BUF - 16) & ~3
        sz = R.choice("wl")
        return [f"cas.{sz} {dreg()},{dreg()},{o}(a5)"]
    if t == 21:
        o = R.randrange(0, BUF - 16) & ~3
        sz = R.choice("bwl")
        return [f"cmp2.{sz} {o}(a5),{dreg()}", f"s{R.choice(['cs', 'eq'])} {o + 12}(a5)"]
    if t == 22:
        p, e = ea("b")
        if "+" in e or "-(" in e:
            p, e = [], f"{R.randrange(0, BUF - 16)}(a5)"
        op = R.choice(["bfextu", "bfexts", "bfffo", "bfins", "bfset", "bfclr", "bfchg", "bftst"])
        if R.random() < 0.5:
            fld = f"{{{R.randrange(0, 32)}:{R.randrange(1, 33) % 32}}}"
            pre = []
        else:
            fld = "{d6:d7}"
            pre = [f"moveq #{R.randrange(0, 60)},d6", f"moveq #{R.randrange(0, 40)},d7"]
        if op in ("bfextu", "bfexts", "bfffo"):
            return p + pre + [f"{op} {e}{fld},{R.choice(['d0', 'd1', 'd2', 'd3', 'd4', 'd5'])}"]
        if op == "bfins":
            return p + pre + [f"bfins {R.choice(['d0', 'd1', 'd2'])},{e}{fld}"]
        return p + pre + [f"{op} {e}{fld}"]
    if t == 23:
        o = R.randrange(4, BUF - 16)
        return [f"lea {o}(a5),a3", f"lea {o + 6}(a5),a4", f"{R.choice(['pack', 'unpk'])} -(a3),-(a4),#{R.randrange(0, 65536)}"]
    if t == 24:
        p, e = ea("w")
        return p + [f"move.w {e},ccr", f"move.w sr,{BUF + 8}(a5)", f"move.w ccr,{BUF + 10}(a5)"]
    # t == 25: valores nuevos en los registros, para que haya de todo
    return [f"move.l #{imm('l')},{dreg()}"]


def main():
    out = ["| Generado por gen_t3.py: no editar a mano", "\t.text", "\t.globl\t_start", "_start:",
           "\tlea\tdatos,a5", "\tlea\t0(a5),a0", "\tlea\t64(a5),a1", "\tlea\t128(a5),a2"]
    for i in range(7):
        out.append(f"\tmove.l\t#{R.randrange(-2**31, 2**31)},d{i}")
    # la zona de datos, con bytes al azar
    for i in range(0, BUF + 64, 4):
        out.append(f"\tmove.l\t#{R.randrange(0, 2**32)},{i}(a5)")
    for _ in range(N):
        for l in instr():
            out.append("\t" + l.replace(" ", "\t", 1))
    out += ["\tmovem.l\td0-d7/a0-a4,datos+%d" % (BUF + 128), "\tmove.w\tsr,datos+%d" % (BUF + 192), "\trts",
            "", "\t.data", "\t.even", f"datos:\t.fill\t{BUF + 256},1,0"]
    print("\n".join(out))


if __name__ == "__main__":
    main()
