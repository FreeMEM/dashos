#!/usr/bin/env python3
"""t10: expulsión de bloques del JIT. 700 funciones de 180 instrucciones llamadas por bctrl, tres pasadas: con poco
JIT (make JITMB=1) se expulsan bloques y se vuelve a ellos por el índice rápido (LRUCache). r3 acumula."""
N, K, PASADAS = 700, 180, 3
print("""        .section .text.prueba,"ax"
        .globl prueba
prueba:
        mflr 0
        stw 0,4(1)
        stwu 1,-16(1)
        li 3,0
        li 9,%d
pasada:
        lis 8,tabla@ha
        addi 8,8,tabla@l
        li 10,%d
bucle:
        lwz 11,0(8)
        mtctr 11
        bctrl
        addi 8,8,4
        addic. 10,10,-1
        bne bucle
        addic. 9,9,-1
        bne pasada
        addi 1,1,16
        lwz 0,4(1)
        mtlr 0
        li 0,0
        mtcrf 0xff,0
        mtctr 0
        li 8,0
        li 11,0
        blr""" % (PASADAS, N))
x = 0x12345678
for i in range(N):
    print("f%d:" % i)
    for k in range(K):
        x = (x * 1103515245 + 12345) & 0xffffffff
        c = (x >> 8) & 0x7fff
        op = (x >> 4) % 3
        if op == 0:
            print("        addi 3,3,%d" % c)
        elif op == 1:
            print("        xori 3,3,%d" % c)
        else:
            print("        rlwinm 3,3,%d,0,31" % (1 + c % 31))
    print("        blr")
print("        .section .rodata\ntabla:")
for i in range(N):
    print("        .long f%d" % i)
