| U7: excepciones. Los marcos los escribe Emu68 desde C (M68k_ExceptionEntry.c) y el vector se lee de la tabla de
| VBR; todo eso es memoria del 68k, en big-endian. Un manejador común apunta cada marco (SR, PC, formato y vector) en
| DATOS y vuelve con RTE. Se compara con el JIT big-endian.

	.text
	.globl	_start
_start:
	lea	vectores,a0
	movec	a0,vbr
	lea	datos,a6
	moveq	#0,d7
	| TRAP #0 y #5
	trap	#0
	trap	#5
	| división por cero (formato 2)
	moveq	#0,d1
	move.l	#100,d0
	divu.w	d1,d0
	move.l	d0,(a6)+
	moveq	#0,d1
	move.l	#1000,d0
	divs.l	d1,d0
	| CHK fuera de rango
	move.w	#50,d0
	chk.w	#10,d0
	move.w	#-1,d0
	chk.w	#10,d0
	| ILLEGAL y una línea A que no es llamada nativa (el manejador salta la instrucción)
	illegal
	.word	0xa123
	| TRAPV con V
	move.w	#2,ccr
	trapv
	| TRAPcc
	moveq	#1,d0
	cmpi.l	#1,d0
	trapeq
	| lo que queda del marco del último y el contador
	move.l	d7,(a6)+
	move.l	a6,d0
	lea	datos,a0
	sub.l	a0,d0
	move.l	d0,(a6)+
	| la tabla de vectores, leída desde el 68k
	move.l	vectores+0x80,(a6)+
	rts

manejador:
	move.w	(sp),(a6)+
	move.l	2(sp),(a6)+
	move.w	6(sp),(a6)+
	addq.l	#1,d7
	move.w	6(sp),d6
	andi.w	#0x0fff,d6
	cmpi.w	#0x10,d6
	beq.s	saltar2
	cmpi.w	#0x28,d6
	beq.s	saltar2
	rte
saltar2:
	addq.l	#2,2(sp)
	rte

	.data
	.even
vectores:
	.rept	256
	.long	manejador
	.endr
datos:	.fill	512,1,0
