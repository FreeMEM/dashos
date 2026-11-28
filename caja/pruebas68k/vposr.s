| K7 (docs/CHIPS_LOOP.md): lee VPOSR ($DFF004) D0 x 1000 veces seguidas (D0, el largo de sus argumentos, como desde el
| CLI), para medir en la caja lo que cuesta un acceso a los chips: por el fallo y con la instrucción reescrita. D0 = 0.

	.text
	.globl	_start
_start:
	lea	0xdff000,a0
	move.l	d0,d1
	mulu.l	#1000,d1
	beq.s	2f
1:	move.w	4(a0),d0
	subq.l	#1,d1
	bne.s	1b
2:	moveq	#0,d0
	rts
