| H4: el primer programa 68k de la caja Amiga (en $110000). Suma de 1 a 100.000 y la escribe por la dos.library
| nativa de la caja (PutStr, VPrintf, Output y Write: cada una, una línea A en su tabla de saltos). Al empezar,
| A6 = DOSBase. Devuelve la suma en D0. Los textos, en ASCII: el Amiga es Latin-1 y la consola, UTF-8.

	.text
	.globl	_start
_start:
	moveq	#0,d5
	move.l	#100000,d1
1:	add.l	d1,d5
	subq.l	#1,d1
	bne.s	1b
	move.l	#saludo,d1
	jsr	-948(a6)		| PutStr(saludo)
	lea	args,a0
	move.l	d5,(a0)
	move.l	#formato,d1
	move.l	a0,d2
	jsr	-954(a6)		| VPrintf(formato, args)
	jsr	-60(a6)			| Output()
	move.l	d0,d1
	move.l	#escrito,d2
	move.l	#escrito_fin-escrito,d3
	jsr	-48(a6)			| Write(Output(), escrito, largo)
	move.l	d5,d0
	rts

	.data
saludo:	.asciz	"  [caja-amiga] hola desde un programa 68k, por la dos.library nativa\n"
formato:	.asciz	"  [caja-amiga] suma de 1 a 100000 en el 68k: %ld\n"
escrito:	.ascii	"  [caja-amiga] Write() en Output(): bien\n"
escrito_fin:
	.even
args:	.long	0
