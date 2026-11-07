# Prueba PPC F1h: mftb. El contador de tiempo no puede dar lo mismo en el JIT (CNTVCT de ARM) que en qemu-ppc (el
# reloj de la máquina), así que no se comparan sus valores: solo lo que tiene que cumplir. A 0x00100000:
#   +0  1 si TB avanza a lo largo de un bucle de 200000 vueltas (lectura de 64 bits con mftbu/mftb/mftbu)
#   +4  cuántas de 1000 lecturas seguidas de 64 bits fueron menores que la anterior (0)
#   +8  cuántas de 1000 lecturas de TBL con mfspr 268 fueron menores que la anterior sin que cambiara TBU (0)
#   +12 1 si mfspr 269 (TBU) da lo mismo que mftbu
# Al final, los registros que tuvieron valores del contador quedan a 0.
        .section .text.prueba,"ax"
        .globl prueba
prueba:
        lis     2,0x10
        bl      leetb                   # r5:r6 = TB
        mr      14,5
        mr      15,6
        lis     8,3                     # 196608 vueltas
1:      addi    8,8,-1
        cmpwi   8,0
        bne     1b
        bl      leetb
        subfc   6,15,6                  # (r5:r6) - (r14:r15) > 0 ?
        subfe.  5,14,5
        li      9,0
        blt     2f
        or.     6,6,5
        beq     2f
        li      9,1
2:      stw     9,0(2)
        # 1000 lecturas de 64 bits, cada una >= la anterior
        bl      leetb
        mr      14,5
        mr      15,6
        li      10,0
        li      8,1000
3:      bl      leetb
        cmplw   5,14
        blt     4f
        bgt     5f
        cmplw   6,15
        bge     5f
4:      addi    10,10,1
5:      mr      14,5
        mr      15,6
        addi    8,8,-1
        cmpwi   8,0
        bne     3b
        stw     10,4(2)
        # TBL con mfspr 268, TBU con mfspr 269
        li      10,0
        li      8,1000
        mfspr   15,268
        mftbu   14
6:      mfspr   6,268
        mfspr   5,269
        cmplw   6,15
        bge     7f
        cmplw   5,14                    # TBL bajó: vale si TBU cambió
        bne     7f
        addi    10,10,1
7:      mr      15,6
        mr      14,5
        addi    8,8,-1
        cmpwi   8,0
        bne     6b
        stw     10,8(2)
8:      mftbu   5
        mfspr   7,269
        mftbu   6
        cmplw   5,6
        bne     8b                      # TBU cambió entre medias: otra vez
        li      9,0
        cmplw   5,7
        bne     9f
        li      9,1
9:      stw     9,12(2)
        li      0,0
        li      2,0
        li      5,0
        li      6,0
        li      7,0
        li      8,0
        li      10,0
        li      14,0
        li      15,0
        mtlr    0
        mtxer   0
        mtcrf   0xff,0
        blr

# r5:r6 = TB (TBU:TBL), leída de nuevo si TBU cambia entre medias
leetb:  mftbu   5
        mftb    6
        mftbu   7
        cmplw   5,7
        bne     leetb
        blr
