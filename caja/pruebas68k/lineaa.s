| H4, prueba hostil de la caja (en $120000): una llamada nativa con un número que no existe (la caja la rechaza: D0
| = -1, y el programa sigue) y una línea A que no es $A0FF (excepción 10 del 68k: el programa acaba, la caja sigue).

	.text
	.globl	_start
_start:
	move.l	#1234,d0
	.word	0xa0ff,999		| la función nativa 999 no existe
	cmp.l	#-1,d0
	bne.s	mal
	move.l	#sigue,d1
	jsr	-948(a6)		| PutStr(sigue)
	.word	0xa123			| excepción de línea A: no vuelve
	move.l	#nollega,d1
	jsr	-948(a6)
	rts
mal:	move.l	#malmsg,d1
	jsr	-948(a6)
	rts

	.data
sigue:	.asciz	"  [caja-amiga] funcion 999 rechazada; el programa 68k sigue\n"
nollega:	.asciz	"  [caja-amiga] MAL: la linea A $A123 no paro el programa\n"
malmsg:	.asciz	"  [caja-amiga] MAL: la funcion 999 no dio -1\n"
