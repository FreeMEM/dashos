Ficheros de CMake para las toolchains de Bootlin en `~/.local/opt/toolchains/`.

`bin/` tiene envoltorios con los nombres que espera el CMake de Emu68 para el ROM PPC (`powerpc-linux-gnu-gcc-14`...).
Añaden `-fno-stack-protector -fno-pie -fno-pic -no-pie`, porque Bootlin activa SSP y PIE por defecto.

Compilar Emu68 para QEMU `virt`:

    git clone https://github.com/michalsc/Emu68.git third_party/Emu68
    git -C third_party/Emu68 submodule update --init --recursive
    git -C third_party/Emu68 apply ../../patches/emu68/*.patch
    cmake -G Ninja -S third_party/Emu68 -B build/emu68-virt -DTARGET=virt \
        -DCMAKE_TOOLCHAIN_FILE=$PWD/toolchains/aarch64be.cmake
    PATH=$PWD/toolchains/bin:$PATH ninja -C build/emu68-virt
