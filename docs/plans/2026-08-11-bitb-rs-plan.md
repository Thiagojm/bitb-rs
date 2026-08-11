# Plano de implementação da crate bitb-rs

**Data:** 2026-08-11
**Base:** `docs/specs/2026-08-11-bitb-rs-design.md` aprovada

## Goal

Entregar a crate Rust síncrona `bitb-rs` para ler um BitBabbler White ou Black por vez em Windows e Linux. A crate deve descobrir e abrir o dispositivo, inicializar FTDI/MPSSE, retornar bytes raw completos por padrão, aplicar folding explícito de 0 a 4 apenas em `get_bits_with_fold`, e fornecer `random_u64` e `random_range` raw.

## Out of scope

- Health checks, ENT, FIPS, estatísticas ou certificação da saída.
- Daemon, sockets, kernel entropy pool, hotplug ou reconexão automática.
- Mais de um dispositivo aberto ou mistura de fontes.
- Configuração pública de bitrate, latency USB, chunks ou máscara de geradores.
- API assíncrona, Tauri/Tokio, `rand_core`, macOS e FFI com o C++ oficial.

## Prerequisites

- Rust stable e Rust 1.85.0 instalados.
- Para Windows real: o BitBabbler associado manualmente ao driver WinUSB.
- Para Linux real: acesso do usuário ao dispositivo `0403:7840`, normalmente por regra udev específica.
- Um BitBabbler White e um Black para os testes de integração finais em cada plataforma disponível.
- A árvore local `bit-babbler-0.9/` permanece somente como referência de protocolo GPL; ela não deve ser copiada, movida ou incluída no pacote Cargo.

## Ordered steps

### 1. Inicializar a crate e sua documentação de manutenção

**Arquivos a criar ou modificar:**

- `Cargo.toml`
- `src/lib.rs`
- `README.md`
- `LICENSE`
- `.gitignore`
- `AGENTS.md`
- `TODO.md`
- `docs/PROJECT_CONTEXT.md`
- `docs/DECISIONS.md`

**Ações:**

1. Criar o pacote Cargo `bitb-rs`, edition 2024, com MSRV 1.85.0 e licença MIT.
2. Declarar `rusb` 0.9 como a única dependência de produção.
3. Excluir `bit-babbler-0.9/**` do pacote Cargo e ignorá-lo no Git, preservando a cópia local inalterada como referência GPL.
4. Criar a estrutura vazia dos módulos definidos na spec, mantendo compilação inicial mínima.
5. Registrar em `AGENTS.md` os comandos de validação e as regras: API síncrona, nenhum health check, raw por padrão, único dispositivo por instância e proibição de copiar o código oficial.
6. Registrar fatos, decisões e próximos passos curtos nos arquivos de contexto, sem incluir segredos nem dados de hardware.
7. Escrever o README inicial com a finalidade da crate, matriz Windows/Linux e o aviso de driver/permissões.

**Critérios de aceitação:**

- `cargo metadata --no-deps` reconhece o pacote `bitb-rs`.
- A crate importa como `bitb_rs`.
- `cargo check` passa em Rust stable e 1.85.0.
- Nenhum arquivo de `bit-babbler-0.9/` foi alterado.

### 2. Definir o contrato público de erro, fold e policy interna

**Arquivos a criar ou modificar:**

- `src/error.rs`
- `src/fold.rs`
- `src/policy.rs`
- `src/lib.rs`
- `tests/fold.rs`
- `tests/error.rs`

**Ações:**

1. Implementar `Fold::{Raw, One, Two, Three, Four}`, `Default` para raw e `TryFrom<u8>` com rejeição explícita fora de `0..=4`.
2. Implementar `BitBabblerError` com as categorias aprovadas para descoberta, argumentos, USB, protocolo, inicialização, leitura e intervalos.
3. Manter o contexto de operação USB e a causa `rusb` onde aplicável, sem tornar o texto de `Display` um contrato serializado.
4. Definir constantes privadas finitas para timeout USB, tentativas de sincronização/inicialização, reads vazios e rejection sampling.
5. Reexportar somente os tipos públicos necessários em `lib.rs` e manter documentação de API em todos eles.

**Critérios de aceitação:**

- `Fold::try_from` aceita exatamente os valores 0, 1, 2, 3 e 4.
- Valores inválidos não alcançam nenhuma operação de transporte nos testes.
- Todas as variantes de erro possuem `Display` não vazio e são distinguíveis por match.
- Não há constantes públicas que exponham tuning de USB.

### 3. Criar o adaptador USB `rusb` e a descoberta por serial

**Arquivos a criar ou modificar:**

- `src/transport.rs`
- `src/device.rs`
- `src/error.rs`
- `tests/transport.rs`
- `tests/device_selection.rs`

**Ações:**

1. Definir uma trait privada mínima para enumeração, abertura, configuração, claim, control transfer, bulk write/read, reset e release.
2. Implementar a trait com `rusb`, mantendo o handle e a interface encapsulados.
3. Enumerar candidatos por VID:PID `0403:7840`, abrir cada candidato para ler serial e produto e identificar `White RNG` ou `Black RNG`.
4. Mapear produto desconhecido com o VID:PID esperado para `UnsupportedProduct`; não o omitir silenciosamente.
5. Implementar `DeviceInfo`, `DeviceVariant`, `BitBabbler::list_devices`, `open` e `open_by_serial`.
6. Aplicar as regras zero/um/vários: `NoDevice`, abertura automática com um candidato e `MultipleDevices { count }` quando apropriado.
7. Mapear erros de permissão, ocupado, removido e timeout para categorias estáveis sem apagar a causa de baixo nível.

**Critérios de aceitação:**

- Todos os cenários de seleção são testados com transporte falso: nenhum, um, vários, serial encontrado, serial ausente e serial vazio.
- A API abre somente produtos White/Black reconhecidos.
- Não há estado global ou seleção implícita do “primeiro” quando houver múltiplos dispositivos.
- A trait de transporte não é exposta publicamente.

### 4. Implementar a camada FTDI/MPSSE de inicialização

**Arquivos a criar ou modificar:**

- `src/protocol.rs`
- `src/transport.rs`
- `src/device.rs`
- `tests/protocol_init.rs`

**Ações:**

1. Codificar constantes privadas dos requests FTDI, bitmodes, status bits e comandos MPSSE a partir do comportamento documentado na referência.
2. Validar configuração 1, interface 0, alternate setting 0, dois endpoints bulk, direções e `wMaxPacketSize` antes de inicializar o dispositivo.
3. Implementar reset FTDI, purge, desabilitação de special characters, latency calculada, flow control RTS/CTS e transição para MPSSE.
4. Implementar sincronização com `0xAA` e `0xAB`, incluindo o parse dos bytes de status e a validação de `0xFA` seguido do byte ecoado.
5. Configurar o clock de 2,5 Mbit/s, os pinos/máscara oficiais e o desligamento de loopback.
6. Limitar recuperações e tentativas; converter falha final em `InitializationFailed` ou erro USB/protocolo contextualizado.

**Critérios de aceitação:**

- O transporte falso confirma a ordem e os parâmetros de todos os control/bulk transfers de inicialização.
- Configurações, endpoints, packet sizes e sync inválidos falham com erro tipado antes de uma leitura de entropia.
- A lógica não contém código copiado do C++ oficial.

### 5. Implementar parsing FTDI e leitura raw exata

**Arquivos a criar ou modificar:**

- `src/protocol.rs`
- `src/device.rs`
- `tests/protocol_read.rs`

**Ações:**

1. Construir comandos MPSSE de leitura de 1 a 65.536 bytes, sempre com a contagem codificada como `len - 1` e flush imediato.
2. Implementar escrita integral de comando e bulk reads que aceitem transferências parciais.
3. Criar um parser incremental que remova os dois status bytes de cada pacote FTDI, inclusive quando um bulk read terminar no meio do pacote.
4. Validar modem status esperado para packet size 64/512 e line status permitido.
5. Manter somente dados não processados da transferência atual no read-ahead interno; payload de entropia além da resposta exata do comando é `ProtocolViolation`.
6. Limitar reads vazios, timeouts e recuperação por reset/reinicialização conforme a policy privada.
7. Garantir que qualquer falha descarte o buffer de saída parcial.

**Critérios de aceitação:**

- Vetores simulados para packet size 64 e 512 produzem exatamente os bytes esperados.
- Status bytes, short reads, timeout, desconexão, line status inválido, modem status inválido e excesso de payload são cobertos por regressões.
- Solicitações de 1, 8, 9, 1.024, 65.536 e acima de 65.536 bytes terminam com o tamanho exato ou erro, sem dados parciais.

### 6. Implementar `get_bits`, folding e a API de inteiros

**Arquivos a criar ou modificar:**

- `src/device.rs`
- `src/fold.rs`
- `src/policy.rs`
- `tests/get_bits.rs`
- `tests/random.rs`

**Ações:**

1. Validar em `get_bits` tamanho positivo e byte-aligned antes de reservar memória ou tocar no transporte.
2. Implementar `get_bits` como atalho explícito para `get_bits_with_fold(..., Fold::Raw)`.
3. Implementar folding segmentado: preencher o resultado com o primeiro segmento raw e aplicar XOR dos `2^fold - 1` segmentos seguintes em um buffer temporário limitado ao chunk máximo.
4. Verificar overflow, reserva falível e limites de chunk antes de consumir a saída de cada caminho.
5. Implementar `random_u64` a partir de oito bytes raw em little-endian.
6. Adaptar o rejection sampling testável da `intel_seed` para `random_range`, preservando validação de intervalo e orçamento finito de amostras.
7. Manter todos os métodos de geração síncronos com `&mut self`, sem persistir fold no handle.

**Critérios de aceitação:**

- `get_bits` usa raw sempre.
- `get_bits_with_fold` aceita folds 0 a 4 e retorna exatamente `n_bits / 8` bytes.
- Vetores determinísticos mostram que folding segmentado equivale ao folding do bloco integral para todos os cinco folds.
- Falhas durante qualquer segmento não retornam os dados já obtidos.
- `random_u64` e `random_range` não chamam a camada de folding.
- `random_range` cobre largura 1, potências de dois, largura não potência, proximidade de `u64::MAX`, rejeição e esgotamento.

### 7. Completar robustez, documentação pública e testes de ciclo de vida

**Arquivos a criar ou modificar:**

- `src/lib.rs`
- `src/device.rs`
- `src/error.rs`
- `README.md`
- `tests/drop.rs`
- `tests/hardware.rs`
- `TODO.md`
- `docs/PROJECT_CONTEXT.md`
- `docs/DECISIONS.md`

**Ações:**

1. Implementar `Drop` best-effort: resetar modo FTDI quando seguro e liberar interface/handle sem panic.
2. Documentar que remoção ou reset físico invalida o handle e exige nova abertura pelo consumidor.
3. Criar testes de ciclo de vida para desconexão e `Drop` com transporte falho.
4. Criar testes de hardware separados, claros e seguros, que não afirmem qualidade estatística nem esperem amostras diferentes.
5. Atualizar README com instalação manual do WinUSB, regra udev restrita, seleção por serial, semântica raw/fold, fator de throughput, ausência de health checks, comportamento de erro e orientação `spawn_blocking`.
6. Atualizar contexto e backlog com os comandos executados, plataformas efetivamente verificadas e pendências reais de hardware, distinguindo build local de verificação física.

**Critérios de aceitação:**

- O README permite integrar a crate sem inferir health checks ou reconexão automática.
- Falhas em `Drop` não causam panic.
- Testes de hardware podem ser compilados e reconhecem indisponibilidade/ausência do dispositivo de forma explícita.
- A documentação não contém segredos, dados pessoais de serial ou alegações de certificação.

### 8. Validar artefatos, MSRV e plataformas

**Arquivos a criar ou modificar:**

- `Cargo.lock`
- `TODO.md`
- `docs/PROJECT_CONTEXT.md`

**Ações:**

1. Rodar todos os checks automatizados definidos abaixo em Rust stable.
2. Rodar o mesmo conjunto compatível em Rust 1.85.0 para confirmar o MSRV.
3. Compilar e executar testes sem hardware em Windows e Linux.
4. Em cada plataforma com hardware disponível, validar manualmente White e Black em raw e folds 1–4, além de `random_u64` e `random_range`.
5. Registrar exatamente o que foi confirmado localmente, por plataforma e por hardware; não tratar cross-compilation ou enumeração sem leitura como validação de dispositivo.
6. Parar e registrar a causa se `rusb` ou libusb não compilar em um target suportado; não substituir o backend sem nova decisão de design.

**Critérios de aceitação:**

- Os comandos automatizados passam em stable e MSRV.
- A compilação passa em Windows e Linux.
- A validação física, quando houver hardware, diferencia White e Black e cobre raw/folds 1–4.
- Pendências de hardware indisponível são registradas como pendências, não como sucesso.

## Test plan

Executar a partir da raiz da crate:

```text
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo test --doc
cargo +1.85.0 check --all-targets
cargo +1.85.0 test --all-targets
```

Em PowerShell, usar `cargo` diretamente; não há scripts Node. Em Windows e Linux, executar também os testes de hardware somente com o dispositivo e os drivers/permissões já preparados. Os testes de hardware devem ser separados dos testes puramente determinísticos e devem imprimir claramente quando foram pulados por ausência de dispositivo.

## Risks and open questions

- A primeira validação física pode revelar diferença de endpoint, packet size, driver ou timing entre revisões reais de White e Black. Esses achados devem corrigir o adaptador de protocolo e receber regressão de transporte falso, sem ampliar o escopo para daemon ou health checks.
- No Windows, o principal requisito operacional externo é a associação correta ao WinUSB; a crate apenas relatará o erro de acesso.
- No Linux, permissões udev e a presença de outro processo usando a interface podem impedir abertura; os casos devem ser distinguidos por erro tipado quando o backend permitir.
- A distribuição que vincular libusb estaticamente exige atenção às obrigações da licença LGPL; esse ponto deve ser revisado antes de publicar binários ou a crate.
- Sem hardware White e Black disponível em ambos os sistemas, o software pode ser validado por mocks e compilação, mas a interoperabilidade física em falta deve permanecer explicitamente não confirmada.
