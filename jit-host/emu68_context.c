/*
    Copia de M68K_LoadContext y M68K_SaveContext de Emu68 (src/aarch64/start.c).

    Copyright © 2019 Michal Schulz <michal.schulz@gmx.de>
    https://github.com/michalsc

    This Source Code Form is subject to the terms of the
    Mozilla Public License, v. 2.0. If a copy of the MPL was not distributed
    with this file, You can obtain one at http://mozilla.org/MPL/2.0/.
*/

#include <stdint.h>
#include "M68k.h"

void M68K_LoadContext(struct M68KState *ctx)
{
    __asm__ volatile("mov "CTX_POINTER_ASM", %0\n"::"r"(ctx));

    __asm__ volatile("mov "REG_USP_ASM", %w0"::"r"(ctx->USP));
    __asm__ volatile("mov "REG_MSP_ASM", %w0"::"r"(ctx->MSP));
    __asm__ volatile("mov "REG_ISP_ASM", %w0"::"r"(ctx->ISP));
    __asm__ volatile("mov "CTX_INSN_COUNT_ASM", %0"::"r"(ctx->INSN_COUNT));
    __asm__ volatile("mov "REG_CACR_ASM", %w0"::"r"(ctx->CACR));
    __asm__ volatile("mov "REG_FPSR_ASM", %w0"::"r"(ctx->FPSR));
    __asm__ volatile("mov "REG_FPIAR_ASM", %w0"::"r"(ctx->FPIAR));
    __asm__ volatile("mov "REG_FPCR_ASM", %w0"::"r"(ctx->FPCR));

    __asm__ volatile("ldp w%0, w%1, %2"::"i"(REG_D0),"i"(REG_D1),"m"(ctx->D[0].u32));
    __asm__ volatile("ldp w%0, w%1, %2"::"i"(REG_D2),"i"(REG_D3),"m"(ctx->D[2].u32));
    __asm__ volatile("ldp w%0, w%1, %2"::"i"(REG_D4),"i"(REG_D5),"m"(ctx->D[4].u32));
    __asm__ volatile("ldp w%0, w%1, %2"::"i"(REG_D6),"i"(REG_D7),"m"(ctx->D[6].u32));

    __asm__ volatile("ldp w%0, w%1, %2"::"i"(REG_A0),"i"(REG_A1),"m"(ctx->A[0].u32));
    __asm__ volatile("ldp w%0, w%1, %2"::"i"(REG_A2),"i"(REG_A3),"m"(ctx->A[2].u32));
    __asm__ volatile("ldp w%0, w%1, %2"::"i"(REG_A4),"i"(REG_A5),"m"(ctx->A[4].u32));
    __asm__ volatile("ldp w%0, w%1, %2"::"i"(REG_A6),"i"(REG_A7),"m"(ctx->A[6].u32));

    __asm__ volatile("ldr w%0, %1"::"i"(REG_PC),"m"(ctx->PC));

    __asm__ volatile("ldr d%0, %1"::"i"(REG_FP0),"m"(ctx->FP[0]));
    __asm__ volatile("ldr d%0, %1"::"i"(REG_FP1),"m"(ctx->FP[1]));
    __asm__ volatile("ldr d%0, %1"::"i"(REG_FP2),"m"(ctx->FP[2]));
    __asm__ volatile("ldr d%0, %1"::"i"(REG_FP3),"m"(ctx->FP[3]));
    __asm__ volatile("ldr d%0, %1"::"i"(REG_FP4),"m"(ctx->FP[4]));
    __asm__ volatile("ldr d%0, %1"::"i"(REG_FP5),"m"(ctx->FP[5]));
    __asm__ volatile("ldr d%0, %1"::"i"(REG_FP6),"m"(ctx->FP[6]));
    __asm__ volatile("ldr d%0, %1"::"i"(REG_FP7),"m"(ctx->FP[7]));

    __asm__ volatile("ldrh w1, %0; rbit w2, w1; bfxil w1, w2, 30, 2; mov "REG_SR_ASM", w1"::"m"(ctx->SR):"x1","x2");
    if (ctx->SR & SR_S)
    {
        if (ctx->SR & SR_M)
            __asm__ volatile("mov w%0, "REG_MSP_ASM::"i"(REG_A7));
        else
            __asm__ volatile("mov w%0, "REG_ISP_ASM::"i"(REG_A7));
    }
    else
        __asm__ volatile("mov w%0, "REG_USP_ASM::"i"(REG_A7));
}

void M68K_SaveContext(struct M68KState *ctx)
{
    __asm__ volatile("mov w1, "REG_CACR_ASM"; str w1, %0"::"m"(ctx->CACR):"x1");
    __asm__ volatile("mov x1, "CTX_INSN_COUNT_ASM"; str x1, %0"::"m"(ctx->INSN_COUNT):"x1");
    
    __asm__ volatile("mov w1, "REG_FPSR_ASM"; str w1, %0"::"m"(ctx->FPSR):"x1");
    __asm__ volatile("mov w1, "REG_FPIAR_ASM"; str w1, %0"::"m"(ctx->FPIAR):"x1");
    __asm__ volatile("umov w1, "REG_FPCR_ASM"; strh w1, %0"::"m"(ctx->FPCR):"x1");
    
    __asm__ volatile("stp w%0, w%1, %2"::"i"(REG_D0),"i"(REG_D1),"m"(ctx->D[0].u32));
    __asm__ volatile("stp w%0, w%1, %2"::"i"(REG_D2),"i"(REG_D3),"m"(ctx->D[2].u32));
    __asm__ volatile("stp w%0, w%1, %2"::"i"(REG_D4),"i"(REG_D5),"m"(ctx->D[4].u32));
    __asm__ volatile("stp w%0, w%1, %2"::"i"(REG_D6),"i"(REG_D7),"m"(ctx->D[6].u32));

    __asm__ volatile("stp w%0, w%1, %2"::"i"(REG_A0),"i"(REG_A1),"m"(ctx->A[0].u32));
    __asm__ volatile("stp w%0, w%1, %2"::"i"(REG_A2),"i"(REG_A3),"m"(ctx->A[2].u32));
    __asm__ volatile("stp w%0, w%1, %2"::"i"(REG_A4),"i"(REG_A5),"m"(ctx->A[4].u32));
    __asm__ volatile("stp w%0, w%1, %2"::"i"(REG_A6),"i"(REG_A7),"m"(ctx->A[6].u32));

    __asm__ volatile("str w%0, %1"::"i"(REG_PC),"m"(ctx->PC));

    __asm__ volatile("str d%0, %1"::"i"(REG_FP0),"m"(ctx->FP[0]));
    __asm__ volatile("str d%0, %1"::"i"(REG_FP1),"m"(ctx->FP[1]));
    __asm__ volatile("str d%0, %1"::"i"(REG_FP2),"m"(ctx->FP[2]));
    __asm__ volatile("str d%0, %1"::"i"(REG_FP3),"m"(ctx->FP[3]));
    __asm__ volatile("str d%0, %1"::"i"(REG_FP4),"m"(ctx->FP[4]));
    __asm__ volatile("str d%0, %1"::"i"(REG_FP5),"m"(ctx->FP[5]));
    __asm__ volatile("str d%0, %1"::"i"(REG_FP6),"m"(ctx->FP[6]));
    __asm__ volatile("str d%0, %1"::"i"(REG_FP7),"m"(ctx->FP[7]));

    __asm__ volatile("umov w1, "REG_SR_ASM"; rbit w2, w1; bfxil w1, w2, 30, 2; strh w1, %0"::"m"(ctx->SR):"x1","x2");
    if (ctx->SR & SR_S)
    {
        if (ctx->SR & SR_M)
            __asm__ volatile("mov "REG_MSP_ASM", w%0"::"i"(REG_A7));
        else
            __asm__ volatile("mov "REG_ISP_ASM", w%0"::"i"(REG_A7));
    }
    else
        __asm__ volatile("mov "REG_USP_ASM", w%0"::"i"(REG_A7));
    
    __asm__ volatile("mov w1, "REG_USP_ASM"; str w1, %0"::"m"(ctx->USP):"x1");
    __asm__ volatile("mov w1, "REG_MSP_ASM"; str w1, %0"::"m"(ctx->MSP):"x1");
    __asm__ volatile("mov w1, "REG_ISP_ASM"; str w1, %0"::"m"(ctx->ISP):"x1");
}
