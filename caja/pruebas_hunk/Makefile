# H4: pruebas del cargador hunk (../hunk.c) en el PC, con AddressSanitizer y UBSan: `make -C caja/pruebas_hunk`.
# Las lanza también `make -C kernel prueba`.
CC ?= cc
CFLAGS := -O1 -g -std=c11 -Wall -Wextra -Werror -fsanitize=address,undefined -fno-sanitize-recover=all
B := ../build-pc

all: $(B)/prueba_hunk
	$(B)/prueba_hunk ../pruebas68k/saludo.exe

$(B)/prueba_hunk: prueba_hunk.c ../hunk.c ../hunk.h | $(B)
	$(CC) $(CFLAGS) prueba_hunk.c ../hunk.c -o $@

$(B):
	mkdir -p $(B)

.PHONY: all
