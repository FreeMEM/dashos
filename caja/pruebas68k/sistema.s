| H4: el código 68k de la caja (en $101600, `SISTEMA68K` de caja.h): lo que hace exec en la ROM con las excepciones
| del procesador, y la vuelta de una llamada anidada. Las direcciones de cada pieza son fijas (.org): caja.h las
| nombra. Va en git como sistema.bin (pruebas68k/Makefile), como los demás programas planos.
|
| Exec, con una excepción del procesador (bus, dirección, instrucción ilegal, división entre 0, CHK, TRAPV,
| privilegio, traza, línea A o F, TRAP #n, coma flotante): en modo supervisor, mete el número de la excepción
| (un ULONG) encima del marco y salta a `tc_TrapCode` de la tarea (autodocs de exec, "Exceptions"; RKM Libraries,
| "Exec Tasks: Processor Traps"). Si la tarea no tiene, o no hay tarea, o la pila de supervisor casi se ha
| acabado (una excepción dentro de otra, sin fin), el programa acaba por esa excepción (`trampa_defecto`).

	.text
	.globl	_start
_start:

| --- $101600: despacho. Cada vector (salvo las interrupciones) es "move.l #n,-(sp); jmp despacho". ---
| En la pila: [n.l][marco].
despacho:
	cmpa.l	super_bajo(pc),sp	| ¿queda pila de supervisor? (sp <= el límite: no)
	bls.s	trampa_defecto
	move.l	a0,-(sp)		| [a0][n][marco]
	movea.l	4.w,a0
	movea.l	276(a0),a0		| ExecBase->ThisTask
	cmpa.w	#0,a0
	beq.s	sin_trampa
	movea.l	50(a0),a0		| tc_TrapCode
	cmpa.w	#0,a0
	beq.s	sin_trampa
	move.l	a0,-(sp)		| [destino][a0][n][marco]
	movea.l	4(sp),a0		| a0 como estaba
	move.l	(sp),4(sp)		| [destino][destino][n][marco]
	addq.l	#4,sp
	rts				| a tc_TrapCode con [n][marco]
sin_trampa:
	movea.l	(sp)+,a0
	bra.s	trampa_defecto

| --- $101640: trampa_defecto, el tc_TrapCode de las tareas de la caja (ExecBase->TaskTrapCode). ---
| Su número va a MARCA y sigue en `caida` (abajo): si era la tarea del programa, el programa acaba por esa excepción.
	.org	0x40
trampa_defecto:
	move.w	2(sp),0x101500		| MARCA = n (la palabra baja del ULONG)
	bra.w	caida

| --- $101660: la vuelta de una llamada anidada (caja_ejecutar_68k): el código 68k que la caja llama desde una
| función nativa vuelve aquí con su RTS. La función nativa $FF00 apunta que volvió y deja un 0 encima de la pila: el
| RTS va a la dirección 0, donde acaba el MainLoop de ese nivel. Con varias tareas, si la que llega no es la dueña del
| nivel, la caja la aparca y deja en la pila la vuelta de otra (como en cualquier llamada nativa: lo que sigue es RTS).
	.org	0x60
vuelta_anidada:
	.word	0xa0ff, 0xff00
	rts

| --- $101670: la interrupción de nivel 6 (vector 30): la caja la levanta desde otro hilo (Ctrl-C). La función nativa
| $FF01 la atiende (y entrega las excepciones de la tarea: tc_ExceptCode).
	.org	0x70
interrupcion:
	.word	0xa0ff, 0xff01
	rte

| --- $10167C: el límite de la pila de supervisor (lo escribe la caja). ---
	.org	0x7c
super_bajo:
	.long	0

| --- $101680: caida. La función nativa $FF02 decide: si cayó la tarea del programa, nada (CAMBIO, en $101502, a 0) y
| a COMUN: el programa acaba por esa excepción. Si cayó otra tarea suya, la caja la quita, pone los registros de la
| siguiente y deja encima de la pila de supervisor un marco que vuelve en modo de usuario a un RTS (que la retoma,
| como al volver de cualquier llamada nativa); CAMBIO a 1: RTE.
	.org	0x80
caida:
	.word	0xa0ff, 0xff02
	tst.w	0x101502
	bne.s	1f
	jmp	0x101400		| COMUN
1:	clr.w	0x101502
	rte

| --- $10169C: un RTR suelto: retoma una tarea a la que la caja quitó el testigo en la interrupción de su turno (en su
| pila, el CCR y la PC de donde estaba).
	.org	0x9c
rtr_suelto:
	rtr
