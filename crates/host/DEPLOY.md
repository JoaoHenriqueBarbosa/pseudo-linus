# Deploy do pseudo-linusd no Dokploy

Passo a passo pra subir o daemon na VPS (Dokploy com provider Docker, imagem no registry próprio
`registry.johnenrique.tech`). Nada aqui foi executado contra a VPS: o deploy é decisão do dono.
Credenciais (senha do registry, chave de API do Dokploy) não ficam neste repositório; estão no runbook
pessoal de deploy.

## 0. Antes de tudo

- O backend `kernel` precisa estar integrado no build (ver `STATUS.md`). Sem ele os workers não sobem
  e o `/healthz` fica em 503.
- Docker com BuildKit (padrão desde o Docker 23).
- Um subdomínio decidido, por exemplo `pl.johnenrique.tech`, com o **registro A criado à mão** no
  painel DNS da Hostinger apontando pra `72.60.137.244`. Não há wildcard: sem esse registro o
  Let's Encrypt nunca valida.

## 1. Build e teste local da imagem

Na raiz do repositório:

```bash
docker build -f crates/host/Dockerfile -t registry.johnenrique.tech/pseudo-linus:1 .
```

Teste local com um volume de dados:

```bash
docker volume create pl-data-teste
docker run -d --name pl-teste -p 127.0.0.1:8080:8080 \
  --cpus 1.5 --memory 3g --pids-limit 8192 \
  -v pl-data-teste:/var/lib/pseudo-linus \
  registry.johnenrique.tech/pseudo-linus:1

# Isolamento: Landlock e seccomp precisam valer dentro do container.
docker exec pl-teste pseudo-linusd selftest

# Saúde (200 com os workers prontos).
curl -s http://127.0.0.1:8080/healthz

# Primeira chave de admin (o token aparece uma vez só).
docker exec pl-teste pseudo-linusd admin bootstrap

# Fumaça.
TOKEN=plk_...   # o token impresso acima
curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8080/rpc \
  -d '{"jsonrpc":"2.0","id":1,"method":"sandbox.create","params":{}}'
curl -s -H "Authorization: Bearer $TOKEN" http://127.0.0.1:8080/rpc \
  -d '{"jsonrpc":"2.0","id":2,"method":"exec","params":{"sandbox_id":"sb_...","command":"echo oi | tr a-z A-Z"}}'

docker rm -f pl-teste && docker volume rm pl-data-teste
```

O `docker-compose.yml` em `crates/host/deploy/` faz o mesmo (com `read_only`, `cap_drop: ALL` e
`no-new-privileges`), útil como referência dos limites.

## 2. Publicar a imagem

```bash
docker push registry.johnenrique.tech/pseudo-linus:1
```

Re-push do mesmo tag e Deploy no Dokploy funcionam (o Deploy faz `docker pull`). Pra poder voltar
atrás, empurre também um tag imutável (`:YYYYMMDD-<commit>`) e anote o anterior.

## 3. Criar o app no Dokploy (API, na ordem)

Todas as chamadas são `POST https://dokploy.johnenrique.tech/api/<router>.<procedure>` com o cabeçalho
`x-api-key`. Os corpos com várias linhas vão por arquivo (`-d @payload.json`).

1. **Projeto**: `project.create` `{"name":"pseudo-linus"}`. O ambiente `production` já vem; o
   `environmentId` sai do `project.all`.
2. **App**: `application.create` `{"name":"pseudo-linus","environmentId":"<env>"}`. Guarde o
   `applicationId`.
3. **Provider Docker**: `application.saveDockerProvider`
   `{"applicationId":"<app>","dockerImage":"registry.johnenrique.tech/pseudo-linus:1","username":"docker","password":"<senha do registry>","registryUrl":"registry.johnenrique.tech"}`.
   O `registryUrl` é obrigatório.
4. **Ambiente**: `application.saveEnvironment` com
   `{"applicationId":"<app>","env":"PL_LOG=info\nPL_LOG_FORMAT=json","buildArgs":"","buildSecrets":"","createEnvFile":true}`.
   O resto vem do `/etc/pseudo-linus/config.toml` da imagem; pra mudar algo sem rebuildar, use
   `PL_WORKERS`, `PL_CPUS_PER_WORKER` ou monte outro arquivo e aponte `PL_CONFIG` pra ele.
5. **Volume** (chaves e snapshots persistidos sobrevivem a redeploy): `mounts.create`
   `{"type":"volume","volumeName":"pseudo-linus-data","mountPath":"/var/lib/pseudo-linus","serviceType":"application","serviceId":"<app>"}`.
   O campo é `serviceId`, não `applicationId`.
6. **Recursos**: limite de CPU em 1,5 e de memória em 3 GiB no app (aba Advanced, Resources, ou
   `application.update` com os campos de limite; confira no Swagger o nome e a unidade, o Dokploy usa
   as do Docker: memória em bytes, CPU em nanoCPUs). O `memory_budget` da configuração (2 GiB) fica
   abaixo do limite de memória de propósito. Cada pseudo-processo é uma thread e thread conta como pid
   no cgroup: se o Dokploy expuser limite de pids, use 8192 ou mais.
7. **Domínio**: depois do registro A existir, `domain.create`
   `{"host":"pl.johnenrique.tech","port":8080,"https":true,"applicationId":"<app>","certificateType":"letsencrypt"}`.
   WebSocket (`/ws`) passa pelo Traefik do Dokploy sem configuração extra.
8. **Deploy**: `application.deploy` `{"applicationId":"<app>","title":"pseudo-linus 1"}` e acompanhe
   `application.one?applicationId=<app>` até `applicationStatus` virar `done` (ou `error`).

## 4. Primeira chave de admin

O comando de admin escreve direto no volume e vale com o daemon rodando (sem reiniciar). Pelo
terminal do app no Dokploy (botão Open Terminal) ou por SSH na VPS:

```bash
docker ps --filter name=pseudo-linus --format '{{.Names}}'
docker exec -it <container> pseudo-linusd admin bootstrap --expires 90d
```

Guarde o token (ele não aparece de novo; o arquivo só tem o SHA-256 dele). Daí em diante dá pra
administrar pelo RPC com essa chave:

```bash
curl -s -H "Authorization: Bearer $ADMIN" https://pl.johnenrique.tech/rpc -d '{"jsonrpc":"2.0","id":1,
  "method":"admin.users.create","params":{"name":"alice","quota":{"max_sandboxes":2}}}'
curl -s -H "Authorization: Bearer $ADMIN" https://pl.johnenrique.tech/rpc -d '{"jsonrpc":"2.0","id":2,
  "method":"admin.keys.create","params":{"user":"alice","label":"agente","expires":"30d"}}'
```

Ou pelo comando local: `pseudo-linusd admin user add alice`, `pseudo-linusd admin key create alice
--expires 30d`, `pseudo-linusd admin key list`, `pseudo-linusd admin key revoke <id>`.

## 5. Verificação depois do deploy

```bash
curl -s https://pl.johnenrique.tech/healthz          # {"status":"ok",...}
docker exec <container> pseudo-linusd selftest       # isolamento ok: landlock=partially_enforced (ABI 4), seccomp=on
osh --remote https://pl.johnenrique.tech --key-file ~/.config/pl.key -c 'uname -a; echo $((6*7))'
```

Na VPS (kernel 6.8, Landlock ABI v4) o esperado é `partially_enforced`: as regras de arquivo e de TCP
valem, o escopo de sinais e de sockets abstratos (ABI v6) não existe lá. Se o `selftest` falhar com
Landlock `not_enforced`, o perfil seccomp padrão do Docker da VPS está barrando as syscalls
`landlock_*`; o seccomp do pseudo-linus continua valendo, e a correção é atualizar o Docker ou passar
um perfil seccomp que libere as três syscalls do Landlock.

## 6. Operação

- **Logs**: JSON no stdout do container (`PL_LOG=debug` pra mais detalhe). Queda de worker aparece
  como `worker caiu: ...` com o sinal; o supervisor reinicia com backoff de 200 ms a 30 s.
- **Atualizar**: build, push do mesmo tag, `application.deploy`. O SIGTERM desliga os workers e grava o
  último uso das chaves; sandboxes em memória se perdem num redeploy (só voltam as que tiverem
  snapshot persistido, que fica no volume).
- **Voltar atrás**: `application.saveDockerProvider` com o tag anterior e `application.deploy`.
- **Backup**: o volume `pseudo-linus-data` (`auth.json` e `snapshots/`).
- **Chaves**: expiração padrão de 90 dias; `admin.keys.list` mostra o último uso; `pseudo-linusd admin
  key prune --older-than 30d` limpa registros mortos.
- **Quotas**: `admin.users.update` com `quota` (vale na hora pra peso e teto de CPU do grupo do
  usuário); o padrão de todos fica em `[quota]` do `config.toml`.
