| La entrada de una biblioteca de disco (prueba.library): si alguien la ejecuta como programa, sale con -1 sin hacer
| nada. Lo primero del hunk de código; el RomTag va en el C.
	.text
	.globl	_start
_start:
	moveq	#-1,d0
	rts
