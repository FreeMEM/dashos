| H4: la entrada de saludo.exe, lo primero del hunk de código (gcc pone los textos de saludo.c delante de su código).
	.text
	.globl	_start
_start:
	jmp	_inicio
