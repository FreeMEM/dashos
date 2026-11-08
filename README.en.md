# DashOS

[Español](README.md)

DashOS is a new operating system for ARM computers, starting with the Raspberry Pi 4. It is modern inside and Amiga
outside: its own kernel in Rust, protected memory and all CPU cores, with the layout, ideas and programs of AmigaOS.

AmigaOS 3.x 68k programs run without emulating a whole Amiga. A JIT (a translator that turns 68k code into ARM code
as it runs) executes their code, and their system calls go to native libraries. It is the idea behind Petunia in
AmigaOS 4 and Trance in MorphOS. DashOS will also imitate the Amiga chips, so that games and demos that drive the
hardware directly work too.

The project is at an early stage. There is a kernel that boots and passes its tests in QEMU, but it cannot be used as
a system yet: there is no desktop and no Amiga box. What is done and what is missing is listed below.

The design documents and work logs are in Spanish.

## Why

Original Amiga hardware breaks down more and more, and there are no spare parts. Accelerator boards such as PiStorm
extend its life, but they need an original motherboard: in twenty years they will make no sense. Emulators such as
UAE imitate the whole machine and leave the Amiga inside a window of another system.

DashOS takes another road: a complete system, on hardware you can buy today, where Amiga programs are first-class
citizens next to new native programs.

## Who it is for

- People with Amiga programs, games or habits who want to keep using them on a current machine.
- Amiga programmers: the same [Dash](https://github.com/FreeMEM/FreeMEMLang) program will compile for AmigaOS 3.x
  (68k) and for DashOS (ARM).
- Anyone who wants a small system that can be understood as a whole and always responds.

## What it will have

Everything in this list is decided in the design (`docs/diseño/`). Almost all of it is still to be done: the real
state is in the next section.

**Amiga programs**

- Amiga box: one process where 68k programs and their libraries live, sharing memory as on an Amiga. If a program
  crashes, the box goes down, not the system.
- `Forbid()` only stops the box, not the rest of the system. Strict exec priorities inside.
- Any 68k task can be killed: exec records the owner of every resource.
- Built-in Enforcer, always on, reporting to a log.
- Amiga program windows are DashOS desktop windows.
- Native exec, dos, graphics, intuition, gadtools... libraries. Until they are all there, the ones in the user's
  Kickstart ROM are used.
- 68k MUI and ReAction running inside the box.
- Own chips (Paula, Blitter, Copper, Denise, Agnus, CIA) for games and demos that drive the hardware, with the CPU in
  the JIT and accuracy at the level of a screen line. A500 or A1200 speed. With the user's Kickstart.
- PowerPC box: WarpOS and PowerUP programs, then MorphOS, and AmigaOS 4 last.
- Native `bsdsocket.library` (networking in the box) and, later, a native `amissl.library`.

**Amiga-style layout**

- Volumes, drawers, assigns (`C:`, `S:`, `LIBS:`, `DEVS:`, `FONTS:`, `LOCALE:`, `ENV:`, `ENVARC:`), Prefs,
  `DEVS:Monitors`, language catalogs, icons and Workbench.
- Per-process assigns, in layers (user, packages and system). Read-only `SYS:`.
- File names that ignore case, as on the Amiga, keeping how they were written. Linux programs get folders that
  respect case.
- Amiga file attributes: comment and protection bits.

**Desktop**

- A trusted official compositor, and anyone's desktops on top of it with no privileges: something like Noctalia or
  DMS on top of Niri in Linux, but with per-program permissions.
- Tiled and floating windows at the same time: dragging a window takes it out of the tiling or puts it back.
- Virtual desktops as Amiga "screens" that can be dragged.
- Program menus in the bar, on the right mouse button.
- Material 3 look with colours taken from the wallpaper, and a "classic Amiga" theme.
- The official desktop: a frame with rounded corners, a bar in four styles, a launcher with categories, quick
  settings, a control centre (music with cover art, calendar, notifications, clipboard with images), screen
  recording, a wallpaper picker, and animations with blur.
- Full customisation: editable bar, video wallpapers, themes as packages, third-party modules in Dash. Prefs with
  Use, Save and Cancel.
- Commodities and ARexx. The lock screen is drawn by the compositor, not by the desktop.

**A modern system**

- Microkernel with capabilities (keys): a new program can do nothing except what it is given.
- Users, groups and permissions as in Linux, with no superuser: administrators confirm each action.
- Per-application permissions, asked on first use.
- Killing a program always works.
- Scheduler with deadlines for the desktop, input and sound.
- No swapping to disk: compressed memory.
- Copy-on-write file system with checksums, automatic snapshots, and each user's drawer encrypted with their
  password.
- Base system as a signed image, and A/B updates that roll back by themselves if they fail. Packages with the `Paq`
  command.
- Linux programs in a box with Arch Linux ARM in a virtual machine (DashOS 0.2), with their windows on the desktop.

**Programming**

- Native API with Amiga concepts, 64-bit and built on capabilities, with POSIX next to it.
- User interface engine with the MUI model, and ReAction and GadTools faces on top.
- System applications and libraries are written in Dash.

**Boards**

- Raspberry Pi 4 first, the Pi 3 too, and the Pi 5 later.
- Other ARM computers that boot through UEFI: boards, ARM laptops and, in the long run, Apple Silicon Macs.
- Candidates: THEA500 Mini and THEA1200.

Measurable goals (`docs/diseño/S0-principios.md`), not measured yet: power-on to desktop in under 5 s on a Pi 4,
under 128 MB of memory at idle, and 60 frames per second moving windows with all four cores at 100 %.

## State

Everything done has been tested in QEMU. Nothing has been tested on a real board yet.

### Done

- **Kernel** in Rust, little-endian, in QEMU `raspi4b`: exceptions, device tree, physical memory, MMU, timer and
  interrupts, all four cores, threads with a fair scheduler and a deadline class, isolated processes, keys and
  channels, shared memory, DMA memory, creating, killing and waiting for processes, memory quotas per group.
  Little-endian or big-endian processes, each with its own byte order.
- **Kernel security**: four hardening phases, each closed with an independent audit (`docs/seguridad/`), and hostile
  tests for every fixed bug.
- **Drivers outside the kernel**, as processes: serial console, SD card, USB (keyboard and mouse), VideoCore mailbox
  and a provisional compositor that draws with the CPU on the firmware's screen. They are started by `inicio`, the
  first program.
- **UEFI boot** in QEMU `virt`: with device tree or ACPI, GICv2 or GICv3, firmware screen, keyboard and mouse through
  xHCI, NVMe disk and MSI-X interrupts.
- **68k JIT**: the Emu68 JIT calls native functions from 68k code, and runs on a little-endian CPU by swapping bytes
  (for CPUs without big-endian, such as Apple's). Tested on a test bench under Linux.
- **PowerPC JIT**, first phase: integers, loads and stores, FPU and branches, compared against `qemu-ppc` on a bench.
  The fixes have been sent to Emu68.
- **Design** of the whole system (S0 to S18), with the decisions taken: `docs/diseño/RESUMEN.md`.
- **Dash** compiles programs for DashOS (AArch64), with its own functions for processes, channels, keys and memory.
  That lives in the Dash repository.

### In progress

- **DashFS file system**, based on RedoxFS: the service reads and writes the NVMe disk from DashOS, with power-cut
  tests. Done: folders that ignore case, the Amiga comment and protection bits, a tool that checks and repairs the
  disk, and snapshots (DashFS 4: take, delete and roll back to one). Still to do: each user's encrypted drawer and
  SD card care.

### Missing

In roadmap order (`docs/diseño/S15-hoja-de-ruta.md`):

| Milestone | What |
|---|---|
| H3 | A compositor that speaks Wayland (today's is provisional) |
| H4 | The Amiga box: native exec and dos.library, compared against a real Amiga |
| H5 | The box with windows: graphics, layers, intuition, gadtools; 68k MUI and ReAction |
| H6 | The real Pi 4: boot, network, HDMI, sound, USB over PCIe |
| H7 | Native Dash, user interface engine and desktop; users, permissions and encryption |
| H8 | Packages and safe updates: **DashOS 0.1** |
| H9 | The Linux box: **DashOS 0.2** |

After that: GPU with Mesa, Wi-Fi and Bluetooth, the Pi 5, the Amiga chips (S18) and the PowerPC box (S17), which can
come earlier if there are hands to do them.

## Trying what exists

You need:

- Rust nightly (with `rustup`, which reads the `rust-toolchain.toml` files).
- The [Bootlin](https://toolchains.bootlin.com/) 2026.08-1 `aarch64be` cross compiler in `~/.local/opt/toolchains/`
  (or the `BOOTLIN` variable pointing to its `bin`). See `toolchains/LEEME.md`.
- `qemu-system-aarch64`, and EDK2 for AArch64 at `/usr/share/edk2/aarch64/QEMU_EFI.fd` (or the `EDK2` variable).
- Python 3.

```sh
make -C kernel prueba          # kernel tests in QEMU raspi4b
make -C kernel arranque        # normal boot in QEMU raspi4b, up to "DashOS en marcha"
make -C kernel prueba-virt     # kernel tests in QEMU virt, booting through UEFI
make -C kernel arranque-virt   # UEFI boot with screen, xHCI and NVMe
```

The JIT benches need Emu68 in `third_party/Emu68` with the patches from `patches/emu68/` (how, in
`toolchains/LEEME.md` and `docs/PPC_LOOP.md`), and `qemu-aarch64_be`:

```sh
make -C jit-host run           # a 68k program in the JIT, big-endian
make -C jit-host ORDEN=le run  # the same with the CPU in little-endian (needs qemu-aarch64)
```

## What is in the repository

| Folder | What |
|---|---|
| `kernel/` | The kernel (Rust) and its tests |
| `usuario/` | Library for programs, and the services: `inicio`, console, USB, SD, NVMe, xHCI, compositor... |
| `uefi/` | The UEFI program that loads the kernel |
| `jit-host/` | Test benches for the Emu68 JITs (68k and PowerPC) as Linux programs |
| `patches/emu68/` | Our changes to Emu68 |
| `toolchains/` | CMake files and wrappers for the Bootlin compilers |
| `tools/` | Scripts to boot and check in QEMU |
| `docs/diseño/` | The design, S0 to S18, and its summary |
| `docs/referencia/` | Kernel calls and service protocols |
| `docs/spec/` | Hardware specifications written in a clean room |
| `docs/seguridad/` | Kernel audits |
| `docs/*_LOOP.md` | Work logs |
| `PLAN.md` | The plan and every decision taken |

## How the work is done

- **Clean room.** What is not documented is studied by one person (or session), who writes a specification in prose;
  another writes the code from that specification alone. No code is copied from the Amiga ROM or from projects with
  incompatible licences. The rules are in `AGENTS.md`.
- **An oracle.** The same 68k program runs on an emulated Amiga with the real Kickstart and on DashOS, and the
  outputs are compared.
- **Every change with its test**, and security fixes with a hostile program that tries to break the kernel.

## What never goes into the repository

Kickstart ROMs, AmigaOS, AmigaOS 4 or MorphOS images, or any other commercial software. DashOS will use the ROM each
user legally owns; it does not distribute it.

## Licence

DashOS code is MPL-2.0 (`LICENSE`), the same licence as Emu68. Changes to Emu68 files stay MPL-2.0. RedoxFS, the base
of the file system, is MIT.
