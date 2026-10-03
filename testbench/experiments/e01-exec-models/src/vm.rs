//! Interpretador de bytecode mínimo, de pilha, pra medir o custo do checkpoint num laço quente (H05).
//!
//! O programa calcula `Σ (i*i) mod 7` com `i` descendo de `n` até 1: 16 instruções por iteração, com um
//! salto pra trás no fim. Três posicionamentos do checkpoint:
//!
//! - `MODE_NONE`: sem checkpoint (linha de base);
//! - `MODE_EVERY`: antes de cada instrução (pior caso);
//! - `MODE_BACKEDGE`: só nos saltos pra trás (o que um interpretador real faz: uma checagem por iteração
//!   de laço do programa interpretado).

use std::sync::atomic::{AtomicBool, Ordering};

use crate::model_c::CtxC;

pub const MODE_NONE: u8 = 0;
pub const MODE_EVERY: u8 = 1;
pub const MODE_BACKEDGE: u8 = 2;

#[derive(Clone, Copy, Debug)]
pub enum Op {
    Push(i64),
    Load(u8),
    Store(u8),
    Add,
    Sub,
    Mul,
    Rem,
    Dup,
    Jnz(u32),
    Halt,
}

/// Instruções executadas por iteração do laço do programa.
pub const OPS_PER_ITER: u64 = 16;

pub fn program(n: i64) -> Vec<Op> {
    use Op::*;
    vec![
        Push(n),
        Store(0),
        Push(0),
        Store(1),
        // laço (pc = 4)
        Load(0),
        Load(0),
        Mul,
        Push(7),
        Rem,
        Load(1),
        Add,
        Store(1),
        Load(0),
        Push(1),
        Sub,
        Dup,
        Store(0),
        Jnz(4),
        // fim
        Load(1),
        Halt,
    ]
}

/// Resultado esperado do programa, calculado direto (confere que o interpretador fez o trabalho).
pub fn expected(n: i64) -> i64 {
    let mut acc = 0i64;
    let mut i = n;
    while i != 0 {
        acc += (i * i) % 7;
        i -= 1;
    }
    acc
}

/// O que a instrução fez com o fluxo.
enum Step {
    Next,
    /// Salto pra trás tomado: é aqui que um interpretador real põe o checkpoint.
    Back,
    Done(i64),
}

#[inline(always)]
fn step(op: Op, stack: &mut Vec<i64>, vars: &mut [i64; 4], pc: &mut usize) -> Step {
    match op {
        Op::Push(v) => stack.push(v),
        Op::Load(i) => stack.push(vars[i as usize & 3]),
        Op::Store(i) => vars[i as usize & 3] = stack.pop().unwrap_or(0),
        Op::Add => {
            let b = stack.pop().unwrap_or(0);
            let a = stack.pop().unwrap_or(0);
            stack.push(a.wrapping_add(b));
        }
        Op::Sub => {
            let b = stack.pop().unwrap_or(0);
            let a = stack.pop().unwrap_or(0);
            stack.push(a.wrapping_sub(b));
        }
        Op::Mul => {
            let b = stack.pop().unwrap_or(0);
            let a = stack.pop().unwrap_or(0);
            stack.push(a.wrapping_mul(b));
        }
        Op::Rem => {
            let b = stack.pop().unwrap_or(1);
            let a = stack.pop().unwrap_or(0);
            stack.push(if b == 0 { 0 } else { a.wrapping_rem(b) });
        }
        Op::Dup => {
            let a = *stack.last().unwrap_or(&0);
            stack.push(a);
        }
        Op::Jnz(t) => {
            if stack.pop().unwrap_or(0) != 0 {
                let back = (t as usize) <= *pc;
                *pc = t as usize;
                return if back { Step::Back } else { Step::Next };
            }
        }
        Op::Halt => return Step::Done(stack.pop().unwrap_or(0)),
    }
    *pc += 1;
    Step::Next
}

/// Versão síncrona (modelos A e B, e linha de base fora do kernel). `slow` é o caminho lento do
/// checkpoint, chamado só quando `flag` está ligado.
#[inline(never)]
pub fn run<const MODE: u8>(prog: &[Op], flag: &AtomicBool, slow: &mut dyn FnMut()) -> i64 {
    let mut stack: Vec<i64> = Vec::with_capacity(16);
    let mut vars = [0i64; 4];
    let mut pc = 0usize;
    loop {
        if MODE == MODE_EVERY && flag.load(Ordering::Relaxed) {
            slow();
        }
        match step(prog[pc], &mut stack, &mut vars, &mut pc) {
            Step::Next => {}
            Step::Back => {
                if MODE == MODE_BACKEDGE && flag.load(Ordering::Relaxed) {
                    slow();
                }
            }
            Step::Done(v) => return v,
        }
    }
}

/// Mesmo laço com o modo decidido em tempo de execução: os três modos rodam exatamente o mesmo código de
/// máquina, e a diferença entre eles é só o teste do flag. As versões especializadas por `const` mudam a
/// alocação de registradores e o layout do laço, e esse efeito (medido até ±20%) engole o do checkpoint.
#[inline(never)]
pub fn run_dyn(prog: &[Op], flag: &AtomicBool, slow: &mut dyn FnMut(), mode: u8) -> i64 {
    let every = std::hint::black_box(mode == MODE_EVERY);
    let backedge = std::hint::black_box(mode == MODE_BACKEDGE);
    let mut stack: Vec<i64> = Vec::with_capacity(16);
    let mut vars = [0i64; 4];
    let mut pc = 0usize;
    loop {
        if every && flag.load(Ordering::Relaxed) {
            slow();
        }
        match step(prog[pc], &mut stack, &mut vars, &mut pc) {
            Step::Next => {}
            Step::Back => {
                if backedge && flag.load(Ordering::Relaxed) {
                    slow();
                }
            }
            Step::Done(v) => return v,
        }
    }
}

/// Versão assíncrona com o modo em tempo de execução (ver [`run_dyn`]).
pub async fn run_async_dyn(prog: &[Op], ctx: &CtxC, mode: u8) -> i64 {
    let every = std::hint::black_box(mode == MODE_EVERY);
    let backedge = std::hint::black_box(mode == MODE_BACKEDGE);
    let flag = &ctx.proc().attention;
    let mut stack: Vec<i64> = Vec::with_capacity(16);
    let mut vars = [0i64; 4];
    let mut pc = 0usize;
    loop {
        if every && flag.load(Ordering::Relaxed) {
            ctx.checkpoint_slow().await;
        }
        match step(prog[pc], &mut stack, &mut vars, &mut pc) {
            Step::Next => {}
            Step::Back => {
                if backedge && flag.load(Ordering::Relaxed) {
                    ctx.checkpoint_slow().await;
                }
            }
            Step::Done(v) => return v,
        }
    }
}

/// Versão assíncrona (modelo C): mesmo laço, com `.await` só no caminho lento.
pub async fn run_async<const MODE: u8>(prog: &[Op], ctx: &CtxC) -> i64 {
    let flag = &ctx.proc().attention;
    let mut stack: Vec<i64> = Vec::with_capacity(16);
    let mut vars = [0i64; 4];
    let mut pc = 0usize;
    loop {
        if MODE == MODE_EVERY && flag.load(Ordering::Relaxed) {
            ctx.checkpoint_slow().await;
        }
        match step(prog[pc], &mut stack, &mut vars, &mut pc) {
            Step::Next => {}
            Step::Back => {
                if MODE == MODE_BACKEDGE && flag.load(Ordering::Relaxed) {
                    ctx.checkpoint_slow().await;
                }
            }
            Step::Done(v) => return v,
        }
    }
}
