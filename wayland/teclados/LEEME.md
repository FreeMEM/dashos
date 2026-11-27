# Mapas de teclado de xkb (G2 de `docs/GRAFICOS_LOOP.md`)

El compositor da a cada cliente de Wayland el mapa de su teclado (`wl_keyboard.keymap`, formato `xkb_v1`): el texto
de un mapa de xkb ya compilado, que el cliente lee con libxkbcommon.

- `es.xkb`: la distribución española, compilada en Linux con `xkbcli compile-keymap --layout es` (libxkbcommon y
  xkeyboard-config 2.49). Es el mapa por defecto; se elegirá en `Prefs/Teclado`.
- Los datos salen de **xkeyboard-config**, con licencias permisivas de tipo MIT/X11 de sus autores: el aviso entero
  está en `COPYING.xkeyboard-config`, que va con el mapa.

Para otra distribución: `xkbcli compile-keymap --layout <código> > <código>.xkb`.
