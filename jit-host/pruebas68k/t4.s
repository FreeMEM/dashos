| U7: lo que t1-t3 tocan poco. (A) accesos de palabra y largo a direcciones impares (el 68020 los admite), (B) las
| cargas de 16 bits con signo por cada camino del JIT (en little-endian, LDRSH no vale: carga sin signo, REV16 y
| SXTH), con valores negativos, (C) MOVES, y (D) la FPU con funciones de libm y operandos de memoria en todos los
| formatos. Se compara con el JIT big-endian. Datos en DATOS (a5); los valores de partida en FUENTE (a4).

	.text
	.globl	_start
_start:
	lea	datos,a5
	lea	fuente,a4

	| ---------- A. accesos desalineados ----------
	move.l	1(a4),d0
	move.w	3(a4),d1
	move.l	d0,1(a5)
	move.w	d1,7(a5)
	lea	0x11(a5),a0
	move.l	5(a4),(a0)+
	move.w	9(a4),(a0)+
	move.l	d0,(a0)+
	move.l	-(a0),d2
	move.w	-(a0),d3
	move.l	d2,0x1b(a5)
	move.w	d3,0x1f(a5)
	moveq	#13,d4
	move.l	(a4,d4.l),d5
	move.w	2(a4,d4.l*2),d6
	move.l	d5,0x23(a5)
	move.w	d6,0x29(a5)
	move.l	d0,0x2f(a5)		| cruza un límite de 8 bytes
	move.l	0x2f(a5),0x37(a5)	| de impar a impar
	add.l	d0,0x23(a5)
	sub.w	d1,0x29(a5)
	addq.l	#7,0x2f(a5)
	neg.w	0x37(a5)
	not.l	0x39(a5)
	eori.l	#0x0f0f0f0f,0x3b(a5)
	lsl.w	0x3f(a5)
	asr.w	0x41(a5)
	movem.l	d0-d3,0x43(a5)
	movem.l	0x43(a5),d4-d7
	movem.w	0x43(a5),a0-a3		| con signo, de una dirección impar
	movem.l	a0-a3,0x55(a5)
	lea	0x7d(a5),a0
	movem.l	d4-d7,-(a0)
	movem.w	d4-d7,-(a0)
	move.l	a0,0x7d(a5)
	lea	0x81(a5),a0
	lea	0x91(a5),a1
	move.l	d0,(a0)
	move.l	d0,(a1)
	cmpm.l	(a0)+,(a1)+
	seq	0x99(a5)
	move.w	d1,(a0)
	move.w	#0x1234,(a1)
	cmpm.w	(a0)+,(a1)+
	scs	0x9a(a5)
	addx.l	-(a0),-(a1)
	subx.w	-(a0),-(a1)
	move.l	a0,0x9b(a5)
	move.l	a1,0x9f(a5)
	mulu.l	0x23(a5),d7
	move.l	d7,0xa3(a5)
	move.w	#7,0xa7(a5)
	divs.w	0xa7(a5),d6
	move.l	d6,0xa9(a5)
	move.l	#-5,0xad(a5)
	move.l	#20,0xb1(a5)
	moveq	#3,d2
	cmp2.l	0xad(a5),d2
	scs	0xb5(a5)
	moveq	#30,d2
	cmp2.l	0xad(a5),d2
	scs	0xb6(a5)
	bfins	d0,0xb7(a5){3:21}
	bfextu	0xb7(a5){5:13},d3
	move.l	d3,0xbd(a5)
	| la FPU en direcciones impares
	fmove.d	#0x400921fb54442d18,fp0
	fmove.d	fp0,0xc1(a5)
	fmove.s	fp0,0xc9(a5)
	fmove.x	fp0,0xcd(a5)
	fmove.l	fp0,0xd9(a5)
	fmove.w	fp0,0xdd(a5)
	fmove.d	0xc1(a5),fp1
	fmove.s	0xc9(a5),fp2
	fmove.x	0xcd(a5),fp3
	fadd.l	0xd9(a5),fp1
	fadd.w	0xdd(a5),fp2
	fmovem.x	fp1-fp3,0xe1(a5)
	fmovem.x	0xe1(a5),fp5-fp7
	fmove.d	fp5,0x111(a5)
	fmove.d	fp6,0x119(a5)
	fmove.d	fp7,0x121(a5)

	| ---------- B. cargas de 16 bits con signo ----------
	| NEGS (a3) en datos+0x1000: palabras negativas y positivas, copiadas de FUENTE. a3 queda en medio para que haya
	| desplazamientos negativos, positivos pequeños, grandes (>= 8192, por registro) e impares.
	lea	negs,a2
	lea	0x1000(a5),a3
	moveq	#15,d0
1:	move.l	(a2)+,(a3)+
	dbra	d0,1b
	lea	0x1000(a5),a3
	move.l	#0x8001fffe,0x2100(a3)
	move.l	#0xff80ff7f,-0x400(a3)
	move.w	#0x80ff,0x1ff(a3)
	move.w	#0x8765,0x2101(a3)
	lea	0x200(a5),a6		| resultados de B
	| MOVEA.W por cada modo
	movea.w	(a3),a0
	move.l	a0,(a6)+
	movea.l	a3,a1
	movea.w	(a1)+,a0
	move.l	a0,(a6)+
	movea.w	(a1)+,a0
	move.l	a0,(a6)+
	movea.w	-(a1),a0
	move.l	a0,(a6)+
	movea.w	6(a3),a0
	move.l	a0,(a6)+
	movea.w	-2(a3),a0		| ldursh negativo (vale 0 aquí: lo de antes de la tabla)
	move.l	a0,(a6)+
	movea.w	0x1ff(a3),a0		| impar mayor que 255
	move.l	a0,(a6)+
	movea.w	-0x400(a3),a0		| negativo grande
	move.l	a0,(a6)+
	movea.w	0x2100(a3),a0		| >= 8192
	move.l	a0,(a6)+
	movea.w	0x2101(a3),a0
	move.l	a0,(a6)+
	moveq	#3,d1
	movea.w	(a3,d1.l*2),a0
	move.l	a0,(a6)+
	movea.w	4(a3,d1.w*4),a0
	move.l	a0,(a6)+
	moveq	#-2,d1
	movea.w	10(a3,d1.l),a0
	move.l	a0,(a6)+
	movea.w	negs+2,a0		| absoluto largo
	move.l	a0,(a6)+
	movea.w	negsp(pc),a0		| relativo al PC
	move.l	a0,(a6)+
	moveq	#1,d1
	movea.w	negsp(pc,d1.l*2),a0
	move.l	a0,(a6)+
	| indirecto en memoria con desplazamientos de palabra negativos (se leen del flujo de instrucciones)
	move.l	a3,0x1f0(a5)
	lea	0x20(a3),a0
	move.l	a0,0x1f4(a5)
	lea	0x1f8(a5),a1
	movea.w	([-8,a1],6),a0
	move.l	a0,(a6)+
	movea.w	([-4,a1],d1.l*2,-0x10),a0
	move.l	a0,(a6)+
	bra.s	2f
	.even
ptrantes:
	.long	negs+6
2:	movea.w	([ptrantes:w,pc],-2),a0	| desplazamiento base de palabra y negativo
	move.l	a0,(a6)+
	movea.w	([ptrneg:w,pc],d1.l*4,-4),a0
	move.l	a0,(a6)+
	movea.w	([ptrneg,pc]),a0
	move.l	a0,(a6)+
	| ADDA, SUBA, CMPA con palabra
	movea.l	#0x10000,a0
	adda.w	(a3),a0
	move.l	a0,(a6)+
	suba.w	2(a3),a0
	move.l	a0,(a6)+
	cmpa.w	4(a3),a0
	scs	(a6)+
	sgt	(a6)+
	smi	(a6)+
	seq	(a6)+
	| MULS, DIVS, CHK con palabra negativa
	move.l	#1000,d2
	muls.w	(a3),d2
	move.l	d2,(a6)+
	move.l	#-100000,d2
	divs.w	8(a3),d2
	move.l	d2,(a6)+
	move.w	#-1,d2
	moveq	#0,d3
	chk.w	12(a3),d3		| 12(a3) = 0x7fff: dentro
	move.l	d3,(a6)+
	| MOVEM.W a registros (se extiende el signo también en los de datos)
	movem.w	(a3),d0-d3/a0-a1
	movem.l	d0-d3/a0-a1,(a6)
	lea	24(a6),a6
	movea.l	a3,a1
	movem.w	(a1)+,d4-d7
	movem.l	d4-d7,(a6)
	lea	16(a6),a6
	move.l	a1,(a6)+
	movem.w	-0x400(a3),d0-d1
	movem.l	d0-d1,(a6)
	addq.l	#8,a6
	| PACK de memoria (con predecremento)
	lea	2(a3),a0
	lea	0x40(a6),a1
	pack	-(a0),-(a1),#0
	move.b	(a1),(a6)+
	| desplazamientos de memoria con valor negativo
	move.w	(a3),0x300(a5)
	asr.w	0x300(a5)
	asl.w	0x300(a5)
	lea	0x304(a5),a0
	move.w	2(a3),-2(a0)
	asr.w	-(a0)
	move.w	(a0),(a6)+
	move.w	0x300(a5),(a6)+
	| CMP2.W y CHK2.W con límites negativos
	move.l	#0xff00ff80,0x308(a5)	| límites -256 y -128
	move.l	#-200,d2
	cmp2.w	0x308(a5),d2
	scs	(a6)+
	sne	(a6)+
	move.l	#-100,d2
	cmp2.w	0x308(a5),d2
	scs	(a6)+
	move.w	#-128,d2
	cmp2.w	0x308(a5),d2
	seq	(a6)+
	| la FPU con palabra por cada camino de su carga (LINEF)
	movea.l	a3,a0
	fmove.w	(a0)+,fp0
	fmove.w	(a0)+,fp1
	fmove.w	-(a0),fp2
	fadd.w	-2(a3),fp2
	fadd.w	0x1ff(a3),fp0
	fadd.w	0x200(a3),fp1
	fadd.w	-0x400(a3),fp2
	fadd.w	0x2100(a3),fp0
	fadd.w	0x2101(a3),fp1
	fmove.w	(a3,d1.l*2),fp3
	fsub.w	negsp(pc),fp3
	fmul.w	2(a3),fp3
	fcmp.w	(a3),fp3
	fsgt	(a6)+
	fmove.d	fp0,(a6)+
	fmove.d	fp1,(a6)+
	fmove.d	fp2,(a6)+
	fmove.d	fp3,(a6)+

	| ---------- C. MOVES ----------
	moveq	#5,d0
	movec	d0,sfc
	movec	d0,dfc
	lea	0x400(a5),a6
	moves.w	(a3),a0			| con signo
	move.l	a0,(a6)+
	moves.w	2(a3),d0
	move.l	d0,(a6)+
	moves.l	(a3),d1
	move.l	d1,(a6)+
	moves.b	(a3),d2
	move.l	d2,(a6)+
	move.l	#0x11223344,d3
	moves.l	d3,(a6)+
	moves.w	d3,(a6)+
	moves.b	d3,(a6)+
	addq.l	#1,a6
	movea.l	#0x55667788,a0
	moves.l	a0,(a6)+
	moves.w	a0,(a6)+
	lea	0x440(a5),a1
	move.l	#0xaabbccdd,(a1)+
	moves.l	-(a1),d4
	move.l	d4,(a6)+
	moves.l	(a1)+,a2
	move.l	a2,(a6)+
	moves.w	-(a1),a2
	move.l	a2,(a6)+

	| ---------- E. indirecto en memoria: guardados, base suprimida y desplazamientos negativos ----------
	lea	0xa00(a5),a6
	lea	0xa80(a5),a0
	move.l	a0,0x9f0(a5)
	move.l	#0x11223344,d0
	move.l	d0,([datos+0x9f0])		| base suprimida
	move.w	d0,([datos+0x9f0],6)
	move.l	([datos+0x9f0]),(a6)+
	move.w	([datos+0x9f0],6),(a6)+
	lea	0x9f8(a5),a1
	move.l	#0x55667788,d0
	move.l	d0,([-8,a1],8)
	moveq	#2,d1
	move.l	d0,([-8,a1],d1.l*4,0x10)	| posindexado
	move.w	d0,([-8,a1],d1.l*2,-2)
	move.l	d0,([-0x10,a1,d1.l*4],0x20)	| preindexado
	move.l	([-8,a1],8),(a6)+
	move.l	([-8,a1],d1.l*4,0x10),(a6)+
	move.w	([-8,a1],d1.l*2,-2),(a6)+
	move.l	([-0x10,a1,d1.l*4],0x20),(a6)+
	movea.w	([-0x10,a1,d1.l*4],0x22),a2	| con signo
	move.l	a2,(a6)+
	movea.w	([datos+0x9f0],8),a2
	move.l	a2,(a6)+
	add.l	d0,([-8,a1],8)			| leer, operar y guardar
	move.l	([-8,a1],8),(a6)+

	| ---------- D. la FPU: libm y formatos de memoria ----------
	lea	0x800(a5),a6
	lea	fvals,a0
	fetox.d	(a0),fp0		| e^0.5
	fmove.d	fp0,(a6)+
	flogn.s	8(a0),fp0		| ln 3
	fmove.d	fp0,(a6)+
	flog10.x	12(a0),fp0	| log10 1000
	fmove.d	fp0,(a6)+
	flog2.l	24(a0),fp0		| log2 1024
	fmove.d	fp0,(a6)+
	ftwotox.w	30(a0),fp0	| 2^-3
	fmove.d	fp0,(a6)+
	ftentox.b	35(a0),fp0	| 10^2
	fmove.d	fp0,(a6)+
	fsinh.d	(a0),fp0
	fmove.d	fp0,(a6)+
	fcosh.d	(a0),fp0
	fmove.d	fp0,(a6)+
	ftanh.d	(a0),fp0
	fmove.d	fp0,(a6)+
	fatan.d	(a0),fp0
	fmove.d	fp0,(a6)+
	fasin.d	(a0),fp0
	fmove.d	fp0,(a6)+
	facos.d	(a0),fp0
	fmove.d	fp0,(a6)+
	fatanh.d	(a0),fp0
	fmove.d	fp0,(a6)+
	ftan.d	(a0),fp0
	fmove.d	fp0,(a6)+
	fcos.d	(a0),fp0
	fmove.d	fp0,(a6)+
	fsincos.d	(a0),fp1:fp2
	fmove.d	fp1,(a6)+
	fmove.d	fp2,(a6)+
	fetoxm1.d	(a0),fp0
	fmove.d	fp0,(a6)+
	flognp1.d	(a0),fp0
	fmove.d	fp0,(a6)+
	fmove.d	#0x401c000000000000,fp0	| 7
	fmod.d	36(a0),fp0		| 7 mod 2.5
	fmove.d	fp0,(a6)+
	fmove.d	#0x401c000000000000,fp0
	frem.d	36(a0),fp0
	fmove.d	fp0,(a6)+
	fmove.l	fpsr,(a6)+
	fmove.d	#0x3ff8000000000000,fp0	| 1.5
	fscale.w	30(a0),fp0
	fmove.d	fp0,(a6)+
	fgetexp.d	44(a0),fp0
	fmove.d	fp0,(a6)+
	fgetman.d	44(a0),fp0
	fmove.d	fp0,(a6)+
	fintrz.d	44(a0),fp0
	fmove.d	fp0,(a6)+
	fint.d	52(a0),fp0
	fmove.d	fp0,(a6)+
	fabs.s	60(a0),fp0
	fmove.s	fp0,(a6)+
	fneg.x	12(a0),fp0
	fmove.x	fp0,(a6)
	lea	12(a6),a6
	fsqrt.l	24(a0),fp0
	fmove.l	fp0,(a6)+
	fdiv.s	8(a0),fp0
	fmove.d	fp0,(a6)+
	fsgldiv.d	(a0),fp0
	fmove.s	fp0,(a6)+
	fsglmul.s	8(a0),fp0
	fmove.s	fp0,(a6)+
	| empaquetado: leer y escribir con varios factores k
	fmove.p	64(a0),fp3
	fmove.d	fp3,(a6)+
	fmove.p	fp3,(a6){#5}
	lea	12(a6),a6
	moveq	#2,d0
	fmove.p	fp3,(a6){d0}
	lea	12(a6),a6
	| constantes de la ROM de la FPU
	fmovecr	#0x00,fp4
	fmove.d	fp4,(a6)+
	fmovecr	#0x0c,fp4
	fmove.d	fp4,(a6)+
	fmovecr	#0x32,fp4
	fmove.x	fp4,(a6)
	lea	12(a6),a6
	| FMOVEM.X con predecremento; FPIAR. (Emu68 no tiene la lista dinámica de FMOVEM.X ni el factor k negativo del
	| empaquetado, y el k positivo trunca en vez de redondear: no se prueban aquí; no son de U7)
	lea	0x40(a6),a1
	fmovem.x	fp0-fp2,-(a1)
	fmovem.x	(a1)+,fp5-fp7
	fmove.d	fp7,(a6)+
	fmove.l	#0x1234,fpiar
	fmovem.l	fpiar,(a6)+
	fmovem.l	fpcr/fpsr/fpiar,(a6)
	lea	12(a6),a6
	fmovem.l	(a0),fpsr
	fmove.l	fpsr,(a6)+
	rts

	.even
negsp:	.word	0x8001,0xfffe,0x7fff,0xff80
ptrneg:	.long	negs+4

	.data
	.even
fuente:	.byte	0x10,0x21,0x32,0x43,0x54,0x65,0x76,0x87,0x98,0xa9,0xba,0xcb,0xdc,0xed,0xfe,0x0f
	.byte	0x80,0x91,0xa2,0xb3,0xc4,0xd5,0xe6,0xf7,0x08,0x19,0x2a,0x3b,0x4c,0x5d,0x6e,0x7f
	.byte	0x01,0x23,0x45,0x67,0x89,0xab,0xcd,0xef,0xfe,0xdc,0xba,0x98,0x76,0x54,0x32,0x10
negs:	.word	0x8001,0xfffe,0x8000,0xff7f,0x0080,0x7ffe,0x7fff,0xc000
	.word	0xf000,0x8123,0xa5a5,0x5a5a,0xffff,0x0001,0xbeef,0xdead
	.word	0x9000,0x9001,0x9002,0x9003,0x9004,0x9005,0x9006,0x9007
	.word	0xe000,0xe001,0xe002,0xe003,0xe004,0xe005,0xe006,0xe007
fvals:	.long	0x3fe00000,0x00000000	| 0: 0.5 (Double)
	.long	0x40400000		| 8: 3.0 (Float)
	.long	0x40080000,0xfa000000,0x00000000	| 12: 1000.0 (extendido)
	.long	1024			| 24: 1024 (largo)
	.word	0
	.word	-3			| 30: -3 (palabra)
	.byte	0,0,0
	.byte	2			| 35: 2 (byte)
	.long	0x40040000,0x00000000	| 36: 2.5 (Double)
	.long	0xc0937000,0x00000000	| 44: -1244.0 (Double)
	.long	0x4002cccc,0xcccccccd	| 52: 2.35 (Double)
	.long	0xc0490fdb		| 60: -pi (Float)
	.long	0x00020001,0x23456789,0x12345678	| 64: 1.2345678912345678e2 (empaquetado)
	.even
datos:	.fill	0x3200,1,0
