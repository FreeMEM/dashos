| K8 (docs/CHIPS_LOOP.md): el contador hacia atrás de los chips. Borra INTREQ, pone en INTENA SET, INTEN y VERTB y da
| 50.000 vueltas sin tocar los chips: solo el contador del motor los pone al día, en cada fin de cuadro (VERTB, que
| pide el nivel 3; el motor lo guarda y no lo entrega). Con argumentos (D0, su largo, distinto de 0), acaba quitando
| INTEN: con esa escritura el nivel preparado vuelve a 0 enseguida. D0 = 0. Solo para la caja (en un Amiga de verdad,
| con INTEN, VERTB sí llegaría al 68k).

	.text
	.globl	_start
_start:
	lea	0xdff000,a0
	move.w	#0x7fff,0x9c(a0)
	move.w	#0xc020,0x9a(a0)
	move.l	#50000,d1
1:	subq.l	#1,d1
	bne.s	1b
	tst.l	d0
	beq.s	2f
	move.w	#0x4000,0x9a(a0)
2:	moveq	#0,d0
	rts
