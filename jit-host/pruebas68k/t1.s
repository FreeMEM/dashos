| U7: instrucciones del 68020/68040 que tocan la memoria de formas raras. Se ejecuta en $140000 con el JIT
| big-endian y con el little-endian; la memoria y los registros al final deben ser iguales. Datos en DATOS.

	.text
	.globl	_start
_start:
	lea	datos,a5
	| --- tamaños y modos de dirección ---
	move.l	#0x11223344,d0
	move.w	#0x5566,d1
	move.b	#0x77,d2
	move.l	d0,(a5)
	move.w	d1,4(a5)
	move.b	d2,6(a5)
	move.b	d2,7(a5)
	lea	8(a5),a0
	move.l	d0,(a0)+
	move.w	d1,(a0)+
	move.b	d2,(a0)+
	move.b	d2,(a0)+
	move.l	d0,-(a0)
	moveq	#3,d3
	move.l	(a5,d3.l*4),d4
	move.w	2(a5,d3.w*2),d5
	move.b	1(a5,d3.l),d6
	move.l	d4,0x40(a5)
	move.w	d5,0x44(a5)
	move.b	d6,0x46(a5)
	move.l	(a5),0x48(a5)
	move.w	(a5),0x4c(a5)
	move.w	2(a5),0x4e(a5)
	movea.w	4(a5),a1
	move.l	a1,0x50(a5)
	movea.l	(a5),a2
	move.l	a2,0x54(a5)
	| indirecto en memoria del 68020: ([bd,An],Xn,od)
	lea	0x60(a5),a3
	move.l	a3,0x58(a5)
	move.l	#0xcafebabe,0x60(a5)
	move.l	#0xdeadbeef,0x64(a5)
	moveq	#1,d3
	move.l	([0x58,a5],d3.l*4,0),d7
	move.l	d7,0x68(a5)
	move.l	([0x58,a5]),0x6c(a5)
	| relativo al PC
	move.l	tabla(pc),0x70(a5)
	move.w	tabla+4(pc),0x74(a5)
	moveq	#2,d3
	move.w	tabla(pc,d3.w*2),0x76(a5)
	| absoluto
	move.l	d0,datos+0x78
	move.w	datos+0x78,0x7c(a5)
	| --- aritmética sobre memoria ---
	add.l	d0,(a5)
	sub.w	d1,4(a5)
	eor.b	d2,6(a5)
	addq.l	#5,8(a5)
	subq.w	#3,12(a5)
	neg.l	16(a5)
	not.w	20(a5)
	clr.b	22(a5)
	addi.l	#0x01010101,24(a5)
	ori.w	#0xf00f,28(a5)
	andi.b	#0x5a,30(a5)
	eori.l	#0x0f0f0f0f,32(a5)
	cmpi.l	#0x11223344,0x48(a5)
	seq	0x80(a5)
	tst.w	0x4c(a5)
	smi	0x81(a5)
	| desplazamientos de memoria (una palabra)
	move.w	#0x8001,0x82(a5)
	asl.w	0x82(a5)
	move.w	#0x8001,0x84(a5)
	lsr.w	0x84(a5)
	move.w	#0x8001,0x86(a5)
	rol.w	0x86(a5)
	move.w	#0x8001,0x88(a5)
	roxr.w	0x88(a5)
	| bits en memoria
	bset	#3,0x8a(a5)
	bchg	#7,0x8b(a5)
	moveq	#1,d3
	bset	d3,0x8c(a5)
	btst	#1,0x8c(a5)
	sne	0x8d(a5)
	tas	0x8e(a5)
	| BCD y extendidas con predecremento
	move.l	#0x12345678,0x90(a5)
	move.l	#0x87654321,0x94(a5)
	lea	0x94(a5),a0
	lea	0x98(a5),a1
	move.l	#0x11111111,(a1)
	andi.b	#0,ccr
	abcd	-(a0),-(a1)
	abcd	-(a0),-(a1)
	sbcd	-(a0),-(a1)
	lea	0x9c(a5),a0
	lea	0xa4(a5),a1
	move.l	#0xffffffff,0x98(a5)
	move.l	#0x00000001,0x9c(a5)
	move.l	#0x00000001,0xa0(a5)
	move.l	#0xffffffff,0xa4(a5)
	andi.b	#0,ccr
	addx.l	-(a0),-(a1)
	addx.l	-(a0),-(a1)
	subx.w	-(a0),-(a1)
	| CMPM
	lea	(a5),a0
	lea	0x48(a5),a1
	cmpm.l	(a0)+,(a1)+
	seq	0xa8(a5)
	cmpm.w	(a0)+,(a1)+
	seq	0xa9(a5)
	cmpm.b	(a0)+,(a1)+
	seq	0xaa(a5)
	| --- campos de bits ---
	move.l	#0x12345678,0xb0(a5)
	move.l	#0x9abcdef0,0xb4(a5)
	bfextu	0xb0(a5){4:12},d0
	bfexts	0xb0(a5){28:8},d1
	bfffo	0xb0(a5){8:16},d2
	movem.l	d0-d2,0xb8(a5)
	moveq	#-1,d0
	bfins	d0,0xb0(a5){30:6}
	bfclr	0xb0(a5){0:3}
	bfset	0xb4(a5){20:4}
	bfchg	0xb4(a5){1:30}
	bftst	0xb0(a5){0:32}
	smi	0xc4(a5)
	moveq	#5,d3
	moveq	#9,d4
	bfextu	0xb0(a5){d3:d4},d5
	move.l	d5,0xc8(a5)
	| --- MOVEM en todas sus formas ---
	moveq	#1,d0
	moveq	#2,d1
	moveq	#3,d2
	move.l	#0x44444444,d3
	lea	0x100(a5),a0
	movem.l	d0-d3,(a0)
	movem.w	d0-d3,0x10(a0)
	lea	0x30(a0),a1
	movem.l	d0-d3/a0,-(a1)
	movem.w	d0-d3,-(a1)
	movem.l	(a0),d4-d7
	movem.w	0x10(a0),a2-a4
	lea	(a0),a1
	movem.l	(a1)+,d0/d2
	move.l	a1,0x30(a0)
	movem.l	d4-d7/a2-a4,0x40(a0)
	| --- MOVEP ---
	move.l	#0xa1b2c3d4,d0
	movep.l	d0,0x80(a0)
	movep.w	d0,0x89(a0)
	movep.l	0x80(a0),d1
	movep.w	0x89(a0),d2
	movem.l	d1-d2,0x90(a0)
	| --- CAS y CAS2 ---
	move.l	#100,0xa0(a0)
	moveq	#100,d0
	move.l	#200,d1
	cas.l	d0,d1,0xa0(a0)
	seq	0xa4(a0)
	moveq	#5,d0
	cas.l	d0,d1,0xa0(a0)
	move.l	d0,0xa8(a0)
	move.w	#0x1234,0xac(a0)
	move.w	#0x1234,d0
	move.w	#0x4321,d1
	cas.w	d0,d1,0xac(a0)
	move.l	#7,0xb0(a0)
	move.l	#8,0xb4(a0)
	lea	0xb0(a0),a1
	lea	0xb4(a0),a2
	moveq	#7,d2
	moveq	#8,d3
	moveq	#70,d4
	moveq	#80,d5
	cas2.l	d2:d3,d4:d5,(a1):(a2)
	seq	0xb8(a0)
	| --- CMP2 / CHK2 sin salirse ---
	move.l	#10,0xc0(a0)
	move.l	#20,0xc4(a0)
	moveq	#15,d0
	cmp2.l	0xc0(a0),d0
	scs	0xc8(a0)
	moveq	#25,d0
	cmp2.l	0xc0(a0),d0
	scs	0xc9(a0)
	move.w	#-5,0xca(a0)
	move.w	#5,0xcc(a0)
	moveq	#0,d0
	cmp2.w	0xca(a0),d0
	scs	0xce(a0)
	| --- PACK / UNPK ---
	move.w	#0x0304,0xd0(a0)
	lea	0xd2(a0),a1
	lea	0xd4(a0),a2
	move.b	#0,(a2)+
	pack	-(a1),-(a2),#0
	move.b	#0x56,0xd6(a0)
	lea	0xd7(a0),a1
	lea	0xda(a0),a2
	unpk	-(a1),-(a2),#0x3030
	| --- multiplicación y división largas ---
	move.l	#0x12345678,0xe0(a0)
	move.l	#0x9abcdef,d0
	mulu.l	0xe0(a0),d1:d0
	movem.l	d0-d1,0xe4(a0)
	move.l	#-1000,d2
	muls.l	0xe0(a0),d3:d2
	movem.l	d2-d3,0xec(a0)
	move.l	#1000007,0xf4(a0)
	move.l	d0,d4
	moveq	#0,d5
	divu.l	0xf4(a0),d5:d4
	movem.l	d4-d5,0xf8(a0)
	divs.w	0xf6(a0),d2
	move.l	d2,0x100(a0)
	| --- pila: LINK, PEA, BSR, RTD ---
	link	a6,#-16
	move.l	#0x5a5a5a5a,-4(a6)
	move.l	-4(a6),0x104(a0)
	pea	0x12(a5)
	move.l	(sp)+,0x108(a0)
	unlk	a6
	move.l	#0x0bad0bad,-(sp)
	bsr	sub_rtd
	move.l	d0,0x10c(a0)
	| --- SR y CCR en memoria ---
	move.w	sr,0x110(a0)
	move.w	#0x001f,0x112(a0)
	move.w	0x112(a0),ccr
	move.w	ccr,0x114(a0)
	| --- MOVE16 ---
	lea	0x200(a5),a1
	lea	0x300(a5),a2
	moveq	#3,d0
1:	move.l	d0,(a1)+
	mulu.w	#0x1111,d0
	dbra	d0,1b
	lea	0x200(a5),a1
	move16	(a1)+,(a2)+
	| --- FPU ---
	lea	0x400(a5),a0
	fmove.l	#3,fp0
	fmove.s	#0x40490fdb,fp1
	fmove.d	#0x4005bf0a8b145769,fp2
	fmove.l	fp0,(a0)
	fmove.w	fp0,4(a0)
	fmove.b	fp0,6(a0)
	fmove.s	fp1,8(a0)
	fmove.d	fp2,0x10(a0)
	fmove.x	fp2,0x20(a0)
	fmove.p	fp2,0x30(a0){#3}
	fmul.x	fp1,fp2
	fmove.x	fp2,0x40(a0)
	fadd.s	8(a0),fp0
	fmove.d	fp0,0x50(a0)
	fmove.x	0x20(a0),fp3
	fmove.p	0x30(a0),fp4
	fmove.w	4(a0),fp5
	fmove.b	6(a0),fp6
	fdiv.d	0x10(a0),fp3
	fmovem.x	fp0-fp7,0x60(a0)
	fmovem.x	fp3/fp4,-(sp)
	fmovem.x	(sp)+,fp6/fp7
	fmovem.x	fp6/fp7,0x100(a0)
	fmove.l	fpcr,0x120(a0)
	fmove.l	fpsr,0x124(a0)
	fmovem.l	fpcr/fpsr,0x128(a0)
	fcmp.d	0x10(a0),fp2
	fsgt	0x130(a0)
	ftst.x	fp5
	fseq	0x131(a0)
	fmove.d	#0x3fe0000000000000,fp7
	fsin.x	fp7,fp0
	fmove.d	fp0,0x138(a0)
	fsqrt.d	0x10(a0),fp1
	fmove.d	fp1,0x140(a0)
	fint.x	fp2,fp2
	fmove.l	fp2,0x148(a0)
	rts

sub_rtd:
	move.l	4(sp),d0
	rtd	#4

	.even
tabla:	.long	0x01020304
	.word	0x0506,0x0708,0x090a

	.data
	.even
datos:	.fill	2048,1,0
