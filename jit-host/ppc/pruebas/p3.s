# Prueba PPC F1f: llamadas nativas. Una "biblioteca" con tres entradas (opcode 1 + número, y blr) que se llaman
# por la tabla, con bl y con bctrl, en un bucle; los resultados van a 0x00100000.
        .section .text.prueba,"ax"
        .globl prueba
prueba:
        mflr    0
        stw     0,4(1)
        stwu    1,-32(1)
        lis     2,0x10
        li      3,21
        bl      lib1            # 42
        stw     3,0(2)
        li      4,100
        bl      lib2            # 142
        stw     3,4(2)
        li      3,1
        li      4,2
        li      5,3
        li      6,4
        li      7,5
        li      8,6
        li      9,7
        li      10,8
        bl      lib3            # 36
        stw     3,8(2)
        lis     12,lib1@ha
        addi    12,12,lib1@l
        li      11,0
        li      3,1
1:      mtctr   12
        bctrl                   # r3 *= 2, 20 veces
        addi    11,11,1
        cmpwi   11,20
        blt     1b
        stw     3,12(2)
        .long   0x04000063      # sin función: r3 = -1
        stw     3,16(2)
        addi    1,1,32
        lwz     0,4(1)
        mtlr    0
        li      0,0
        li      2,0
        li      12,0
        mtctr   0
        blr
lib1:   .long   0x04000001
        blr
lib2:   .long   0x04000002
        blr
lib3:   .long   0x04000003
        blr
