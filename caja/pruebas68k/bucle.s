| H4, prueba hostil de la caja (en $130000): un bucle sin fin. Lo corta el vigilante de la caja con la interrupción
| de nivel 7; D0 cuenta las vueltas.

	.text
	.globl	_start
_start:
	moveq	#0,d0
1:	addq.l	#1,d0
	bra.s	1b
