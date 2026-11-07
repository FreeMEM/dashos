# Prueba PPC F1h: código que se modifica a sí mismo, con dcbst, sync, icbi e isync. A 0x00100000:
#   +0..+39   una rutina en 0x00120000 (li 3,N; blr) que se reescribe y se llama 10 veces: r3 de cada llamada
#   +40..+59  una instrucción del propio programa (li 4,N) dentro de un bucle de 5 vueltas, ya traducida en la
#             vuelta anterior, que se cambia en cada vuelta antes de llegar a ella: r4 de cada vuelta
#   +60..+79  la rutina, llamada con bl desde un bloque fijo (que el JIT puede meter dentro del suyo), con dos
#             instrucciones cambiadas cada vez (li 3,N y addi 3,3,N+1): r3 de cada llamada
        .section .text.prueba,"ax"
        .globl prueba
prueba:
        mflr    0
        lis     2,0x10
        lis     9,0x12                  # la rutina, en 0x00120000
        lis     11,0x4e80
        ori     11,11,0x0020            # blr
        stw     11,4(9)
        li      8,0
1:      mulli   10,8,7
        addi    10,10,3
        oris    10,10,0x3860            # li 3,(8 * 7 + 3)
        stw     10,0(9)
        dcbst   0,9
        sync
        icbi    0,9
        isync
        mtctr   9
        bctrl
        slwi    12,8,2
        stwx    3,2,12
        addi    8,8,1
        cmpwi   8,10
        blt     1b

        lis     9,objetivo@ha
        addi    9,9,objetivo@l
        li      8,0
2:      mulli   10,8,11
        addi    10,10,1
        oris    10,10,0x3880            # li 4,(8 * 11 + 1)
        cmpwi   8,0
        beq     3f                      # la primera vuelta, con la instrucción como está (li 4,-1)
        stw     10,0(9)
        dcbst   0,9
        sync
        icbi    0,9
        isync
3:      nop
objetivo:
        li      4,-1
        slwi    12,8,2
        addi    12,12,40
        stwx    4,2,12
        addi    8,8,1
        cmpwi   8,5
        blt     2b

        lis     9,rutina@ha
        addi    9,9,rutina@l
        li      8,0
4:      mulli   10,8,5
        oris    11,10,0x3860            # li 3,(8 * 5)
        stw     11,0(9)
        addi    10,10,1
        oris    11,10,0x3863            # addi 3,3,(8 * 5 + 1)
        stw     11,4(9)
        dcbst   0,9
        sync
        icbi    0,9
        addi    11,9,4
        dcbst   0,11
        sync
        icbi    0,11
        isync
        bl      rutina
        slwi    12,8,2
        addi    12,12,60
        stwx    3,2,12
        addi    8,8,1
        cmpwi   8,5
        blt     4b

        mtlr    0
        li      0,0
        li      2,0
        li      9,0
        li      11,0
        mtctr   0
        mtcrf   0xff,0
        blr

rutina: li      3,0
        addi    3,3,0
        blr
