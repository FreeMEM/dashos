| U7: el rastreo de banderas del JIT (M68K_GetSRMask) sigue los dos caminos de un DBcc para saber si una bandera se
| usa después. Leía el desplazamiento del DBcc sin girarlo: en little-endian seguía un camino que no es. Aquí, el
| desplazamiento $0400 girado es $0004, que cae en un ADD que pone X; el destino de verdad usa X (ADDX). Si el
| rastreo se equivoca, el JIT no calcula la X de ADDQ y ADDX da otra cosa.

	.text
	.globl	_start
_start:
	lea	datos,a5
	moveq	#5,d4
	moveq	#-1,d1
	moveq	#0,d2
	moveq	#0,d3
	moveq	#1,d7
	addq.l	#1,d1		| X = 1
	dbf	d4,destino	| salta (desplazamiento $0400)
	nop
	add.l	d7,d7		| aquí cae el desplazamiento girado: pone X
	move.l	#0xdead,(a5)
	rts
	.fill	0x3f2,1,0
destino:
	addx.l	d2,d3		| d3 = X = 1
	move.l	d3,(a5)
	| lo mismo con DBRA hacia atrás, en un bucle: X de ADD.L viva hasta el ADDX de la vuelta siguiente
	moveq	#9,d4
	moveq	#0,d5
	moveq	#0,d6
	move.l	#0x80000000,d0
1:	addx.l	d6,d5
	add.l	d0,d0
	dbra	d4,1b
	move.l	d5,4(a5)
	rts

	.data
datos:	.fill	64,1,0
