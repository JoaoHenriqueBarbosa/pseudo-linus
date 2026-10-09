//! De onde vêm os bytes de uma URL. A produção usa o cliente HTTP do `ul-net`; os testes injetam uma fita gravada.
//! É um ponto de injeção de dados, não de comportamento: o que o navegador faz com a resposta é o mesmo nos dois casos.

use std::rc::Rc;

use ul_net::net::client::{self, FetchError, RequestSpec, Response};

/// Busca `url` (absoluta), seguindo redirecionamentos.
pub type Fetcher = Rc<dyn Fn(&str, &RequestSpec<'_>) -> Result<Response, FetchError>>;

/// O fetcher de produção: a pilha de rede do sandbox.
pub fn network() -> Fetcher {
    Rc::new(client::request)
}
