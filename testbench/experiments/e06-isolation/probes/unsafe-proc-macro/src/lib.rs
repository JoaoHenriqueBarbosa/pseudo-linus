//! Sonda do E06 (H20): proc macros de função que devolvem `unsafe { from_utf8_unchecked(<entrada>) }`.
//! A única diferença entre elas é o span dado aos tokens gerados (o `unsafe`, as chaves e o caminho):
//!
//! - `call_site`: o padrão do `quote!`. Higiene transparente, mas o contexto de sintaxe é o da expansão.
//! - `mixed_site`: o que `macro_rules!` usa pra variáveis locais.
//! - `input_span`: o span do primeiro token da entrada, que o usuário escreveu. Para o compilador é
//!   como se o `unsafe` estivesse no código de quem chamou. Serve de controle positivo do `forbid`.
//! - `located_at_input`: higiene e contexto de `call_site`, mas linha e coluna apontando pra entrada.
//!
//! A crate não tem código unsafe compilado: o `unsafe` só existe como token produzido.

use proc_macro::TokenStream;
use proc_macro2::{Span, TokenStream as TokenStream2};
use quote::{quote, quote_spanned};

fn first_span(input: &TokenStream2) -> Span {
    input.clone().into_iter().next().map(|t| t.span()).unwrap_or_else(Span::call_site)
}

/// (b) Span `call_site`.
#[proc_macro]
pub fn utf8_unchecked_call_site(input: TokenStream) -> TokenStream {
    let bytes = TokenStream2::from(input);
    quote!(unsafe { ::core::str::from_utf8_unchecked(#bytes) }).into()
}

/// (c) Span `mixed_site`.
#[proc_macro]
pub fn utf8_unchecked_mixed_site(input: TokenStream) -> TokenStream {
    let bytes = TokenStream2::from(input);
    quote_spanned!(Span::mixed_site()=> unsafe { ::core::str::from_utf8_unchecked(#bytes) }).into()
}

/// Controle positivo: os tokens gerados levam o span da entrada (contexto de sintaxe de quem chamou).
#[proc_macro]
pub fn utf8_unchecked_input_span(input: TokenStream) -> TokenStream {
    let bytes = TokenStream2::from(input);
    let span = first_span(&bytes);
    quote_spanned!(span=> unsafe { ::core::str::from_utf8_unchecked(#bytes) }).into()
}

/// Contexto de `call_site` com a localização da entrada.
#[proc_macro]
pub fn utf8_unchecked_located_at_input(input: TokenStream) -> TokenStream {
    let bytes = TokenStream2::from(input);
    let span = Span::call_site().located_at(first_span(&bytes));
    quote_spanned!(span=> unsafe { ::core::str::from_utf8_unchecked(#bytes) }).into()
}
