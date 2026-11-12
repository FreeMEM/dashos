| H4, prueba hostil de la caja (en $140000): escribe en la dirección 4, donde está el puntero a ExecBase, en la página 0
| de solo lectura. El kernel termina la caja (sin el Enforcer de la caja, que aún no está).

	.text
	.globl	_start
_start:
	move.l	#0x12345678,0x4.w
	move.l	#nollega,d1
	jsr	-948(a6)
	rts

	.data
nollega:	.asciz	"  [caja-amiga] MAL: escribio en la pagina 0 y sigue\n"
