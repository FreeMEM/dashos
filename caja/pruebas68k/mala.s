| H4, prueba hostil de la caja (en $140000): escribe fuera de la memoria del 68k. El kernel termina la caja.

	.text
	.globl	_start
_start:
	move.l	#0xdead0000,a0
	move.l	#0x12345678,(a0)
	move.l	#nollega,d1
	jsr	-948(a6)
	rts

	.data
nollega:	.asciz	"  [caja-amiga] MAL: escribio fuera de su memoria y sigue\n"
