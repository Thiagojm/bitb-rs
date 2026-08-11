# Design da crate bitb-rs

**Data:** 2026-08-11
**Status:** aprovado
**Escopo:** acesso síncrono a um BitBabbler White ou Black por vez, em Windows e Linux

## 1. Objetivo

Criar a crate Rust `bitb-rs`, importada como `bitb_rs`, para acessar diretamente dispositivos TNRG BitBabbler White e Black. A API deve seguir os contratos úteis da crate `intel_seed`: núcleo síncrono, erros tipados, tamanhos pedidos em bits, resultado completo ou erro, `random_u64` e `random_range` uniforme.

A diferença funcional é o folding opcional somente em `get_bits_with_fold`. A coleta padrão é sempre raw (`fold = 0`). A crate não executa health checks, testes estatísticos, certificação ou bloqueio da saída por qualidade.

## 2. Requisitos aprovados

- Suportar Windows e Linux na primeira versão.
- Comunicar diretamente com o dispositivo por USB, sem executar `seedd` e sem FFI com o C++ oficial.
- Suportar as variantes White e Black com uma única implementação de protocolo.
- Operar com um dispositivo por instância e sem estado global.
- Abrir automaticamente quando houver exatamente um dispositivo compatível.
- Retornar erro quando `open()` encontrar mais de um dispositivo; nesse caso o consumidor deve selecionar pelo serial.
- Entregar dados raw por padrão.
- Aceitar fold explícito somente entre 0 e 4.
- Aplicar folding somente quando o consumidor chamar `get_bits_with_fold`.
- Manter `random_u64` e `random_range` sempre raw.
- Manter bitrate, latency USB e máscara de geradores como detalhes internos com os valores oficiais.
- Em desconexão ou reinício, retornar erro tipado; o consumidor reabre o dispositivo.
- Não implementar monitor de hotplug ou reconexão automática.
- Não retornar dados parciais em caso de falha.

## 3. Não objetivos

- Health checks, ENT, FIPS ou qualquer aprovação estatística da saída.
- Daemon, serviço do sistema, sockets, UDP ou saída contínua para stdout.
- Alimentação do pool de entropia do kernel.
- Uso simultâneo, pooling ou combinação de vários dispositivos.
- Tuning público de bitrate, latency, tamanho de transferência ou geradores.
- Monitoramento de hotplug ou reconexão automática.
- API assíncrona, Tokio ou Tauri dentro da crate.
- Integração com `rand_core` na primeira versão.
- macOS ou outros sistemas como plataformas oficialmente suportadas.
- Reuso de código GPL do pacote oficial.

## 4. Base técnica observada

O pacote oficial identifica ambos os modelos pelo VID:PID `0403:7840` e os diferencia pelos descritores de produto:

- `White RNG`;
- `Black RNG`.

Os dois usam o mesmo transporte FTDI/MPSSE. O White possui quatro geradores e o Black um gerador. O código oficial escolhe fold 1 para White e fold 3 para Black, mas essa política não será reproduzida: nesta crate o padrão é sempre raw, independentemente da variante.

O protocolo observado usa configuração USB 1, interface 0, alternate setting 0, dois endpoints bulk, bitrate padrão de 2,5 Mbit/s e comandos de leitura MPSSE limitados a 65.536 bytes por solicitação. Cada pacote recebido do FTDI possui dois bytes iniciais de status que não fazem parte da entropia.

As referências principais do pacote oficial são:

- `bit-babbler-0.9/include/bit-babbler/usbcontext.h` para enumeração, descritores, abertura e claim;
- `bit-babbler-0.9/include/bit-babbler/ftdi-device.h` para control transfers, bulk I/O, MPSSE e status FTDI;
- `bit-babbler-0.9/include/bit-babbler/secret-source.h` para identificação dos modelos, configuração e leitura;
- `bit-babbler-0.9/include/bit-babbler/qa.h` para a semântica matemática do folding.

## 5. Transporte USB escolhido

A implementação usará `rusb` 0.9, wrapper Rust seguro sobre libusb. Essa opção foi escolhida porque oferece uma API síncrona que corresponde diretamente às operações do código oficial: enumeração, descritores, configuração, claim, control transfers, bulk transfers, reset e clear halt.

Alternativas rejeitadas:

- `nusb`: tecnicamente viável e puro Rust, mas sua abstração orientada a filas de transferências adiciona adaptação sem benefício necessário para esta primeira versão.
- FFI ou execução de `seedd`: arrastaria código GPL, daemon, pools, threads e health checks para uma biblioteca que não precisa deles.
- Backends próprios por sistema operacional: duplicariam usbfs e WinUSB sem justificativa.

No Windows, o dispositivo deve estar associado ao driver WinUSB. A crate não instala nem substitui drivers. No Linux, o usuário precisa ter permissão sobre o dispositivo, normalmente por regra udev restrita a `0403:7840`. A crate não instala regras do sistema.

## 6. API pública

O manifesto terá:

```toml
[package]
name = "bitb-rs"
```

O nome de importação Rust será `bitb_rs`.

### 6.1 Tipos públicos

```rust
pub struct BitBabbler { /* campos privados */ }

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceVariant {
    White,
    Black,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
#[repr(u8)]
pub enum Fold {
    #[default]
    Raw = 0,
    One = 1,
    Two = 2,
    Three = 3,
    Four = 4,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceInfo {
    pub variant: DeviceVariant,
    pub serial: String,
    pub product: String,
    pub bus_number: u8,
    pub device_address: u8,
}
```

`Fold` implementará `TryFrom<u8>` e aceitará somente valores entre 0 e 4. Um valor externo inválido deve falhar antes de qualquer acesso ao dispositivo.

### 6.2 Operações públicas

```rust
impl BitBabbler {
    pub fn list_devices() -> Result<Vec<DeviceInfo>, BitBabblerError>;

    pub fn open() -> Result<Self, BitBabblerError>;

    pub fn open_by_serial(serial: &str) -> Result<Self, BitBabblerError>;

    pub fn device_info(&self) -> &DeviceInfo;

    pub fn get_bits(&mut self, n_bits: usize)
        -> Result<Vec<u8>, BitBabblerError>;

    pub fn get_bits_with_fold(&mut self, n_bits: usize, fold: Fold)
        -> Result<Vec<u8>, BitBabblerError>;

    pub fn random_u64(&mut self) -> Result<u64, BitBabblerError>;

    pub fn random_range(&mut self, range: std::ops::Range<u64>)
        -> Result<u64, BitBabblerError>;
}
```

Não haverá `close()`. A liberação ocorrerá por RAII/`Drop`.

## 7. Descoberta e seleção

`list_devices` enumera dispositivos com VID:PID `0403:7840` e abre os candidatos o suficiente para ler produto e serial. Modelos White e Black são retornados normalmente. Um candidato com o VID:PID esperado e produto desconhecido produz `UnsupportedProduct`, em vez de ser silenciosamente ocultado. Falha de permissão, dispositivo ocupado ou erro ao ler os descritores também deve ser reportado.

`open()` segue estas regras:

1. nenhum dispositivo reconhecido: `NoDevice`;
2. exatamente um: abre esse dispositivo;
3. mais de um: `MultipleDevices { count }`.

`open_by_serial` exige serial não vazio, procura correspondência exata e retorna `DeviceNotFound` quando não houver correspondência. Produtos diferentes de `White RNG` e `Black RNG`, mesmo com o VID:PID esperado, não podem ser abertos e produzem `UnsupportedProduct`.

A crate não impõe singleton global. Cada `BitBabbler` representa exatamente um handle, e o consumidor assume a responsabilidade de não manter mais de um aberto.

## 8. Estado e concorrência

`BitBabbler` possui o handle USB, metadados do dispositivo, endpoints, tamanho máximo de pacote, line status e buffer interno de read-ahead.

- O tipo não implementa `Copy` nem `Clone`.
- Operações que consomem o fluxo usam `&mut self`.
- Não há mutex global nem leitura concorrente no mesmo handle.
- A crate não promete `Sync` como parte do contrato público.
- O consumidor pode mover chamadas bloqueantes para `spawn_blocking`.
- A crate não executa callbacks nem cria threads em background.

## 9. Inicialização FTDI/MPSSE

A abertura executa, em ordem:

1. validar VID:PID, produto, configuração, interface, alternate setting e endpoints;
2. selecionar configuração USB 1;
3. reivindicar interface 0;
4. resetar o FTDI;
5. drenar dados pendentes;
6. desabilitar event e error characters;
7. configurar o latency timer calculado pelo algoritmo oficial;
8. configurar flow control RTS/CTS;
9. alternar bitmode para reset e depois MPSSE;
10. aguardar o tempo de estabilização;
11. sincronizar o MPSSE com `0xAA` e `0xAB`, esperando `0xFA` e o comando ecoado após os status bytes;
12. desabilitar clock divide-by-5, adaptive clock e three-phase clock;
13. configurar o divisor para 2,5 Mbit/s;
14. configurar pinos e a máscara oficial de geradores;
15. desabilitar loopback.

Todos os loops de inicialização e sincronização terão limites internos finitos. Os limites não fazem parte da API pública na primeira versão.

## 10. Leitura do fluxo

A leitura de dados raw:

1. divide a quantidade necessária em comandos MPSSE de no máximo 65.536 bytes;
2. escreve o comando integralmente, aceitando que o backend conclua a escrita em mais de uma transferência;
3. realiza bulk reads até obter a quantidade pedida;
4. remove os dois bytes de modem/line status do início de cada pacote USB FTDI;
5. valida modem status e rejeita line status inesperado;
6. mantém no buffer privado somente bytes ainda não processados da transferência correspondente ao comando atual;
7. exige que nenhum byte de entropia permaneça após completar o tamanho exato do comando e trata payload excedente como violação de protocolo;
8. limita timeouts, reads vazios e tentativas de recuperação;
9. entrega os bytes ao chamador apenas quando o resultado estiver completo.

Em erro após progresso parcial, o buffer parcial é descartado.

## 11. Semântica dos métodos aleatórios

### 11.1 `get_bits`

`get_bits(n_bits)` é equivalente a `get_bits_with_fold(n_bits, Fold::Raw)`.

Regras:

- `n_bits` deve ser maior que zero;
- `n_bits` deve ser divisível por 8;
- o retorno contém exatamente `n_bits / 8` bytes;
- não há limite artificial de aplicação dentro da crate;
- o consumidor deve impor seus próprios limites de tamanho;
- a reserva do resultado é falível e ocorre antes do consumo relevante de dados.

### 11.2 `get_bits_with_fold`

Para uma saída de `N` bytes e fold `F`, a crate lê `2^F` segmentos consecutivos de `N` bytes. O primeiro segmento preenche o resultado; cada segmento seguinte é combinado por XOR byte a byte com o resultado.

Esse processamento é equivalente ao folding sucessivo do bloco integral e usa apenas:

- o buffer final de `N` bytes;
- um buffer temporário de leitura limitado ao tamanho máximo de chunk.

O fold não é guardado no handle. Cada chamada declara explicitamente o fold, evitando estado residual entre solicitações.

### 11.3 `random_u64`

Obtém oito bytes raw e os interpreta em little-endian, mantendo a convenção de empacotamento usada por `intel_seed`. Folding não é aplicado.

### 11.4 `random_range`

Usa `random_u64` e rejection sampling limitado para produzir valor uniforme no intervalo semiaberto `[start, end)`, sem viés de módulo. Intervalo vazio ou invertido falha antes de consumir dados. Folding não é aplicado.

## 12. Modelo de erros

`BitBabblerError` deve permitir tratamento programático. As mensagens de `Display` servem para diagnóstico e não constituem protocolo estável de IPC.

Variantes previstas:

```text
NoDevice
MultipleDevices { count }
DeviceNotFound { serial }
MissingSerial
UnsupportedProduct { product }
ZeroBitLength
BitLengthNotByteAligned { requested_bits }
InvalidFold { value }
InvalidRange { start, end }
AllocationFailed { requested_bits }
PermissionDenied
DeviceBusy
DeviceDisconnected
TransferTimeout { operation }
Usb { operation, source }
ProtocolViolation { operation }
InitializationFailed { attempts }
ReadRetriesExhausted { attempts }
RangeSamplingExhausted { attempts }
```

Erros de `rusb` serão preservados como fonte quando aplicável e mapeados para categorias estáveis nos casos que o consumidor precisa distinguir. Nenhuma variante contém dados aleatórios parciais.

## 13. Recuperação e encerramento

Erros transitórios de inicialização e reads vazios recebem tentativas internas limitadas, baseadas no comportamento oficial. Timeout permanente, dispositivo removido, interface ocupada e violação de protocolo são devolvidos ao consumidor.

A crate não reenumera nem reabre automaticamente. Depois de desconexão ou reset que invalide o handle, o consumidor descarta a instância e chama `open` ou `open_by_serial` novamente.

Em `Drop`, a crate tenta:

1. drenar dados pendentes quando seguro;
2. restaurar o bitmode de reset;
3. resetar o FTDI quando o handle ainda for válido;
4. liberar a interface e fechar o handle.

Falhas em `Drop` não provocam panic.

## 14. Organização interna

```text
src/
  lib.rs
  device.rs
  error.rs
  fold.rs
  policy.rs
  protocol.rs
  transport.rs
```

- `lib.rs`: documentação da crate e reexports públicos.
- `device.rs`: API pública, seleção e ciclo de vida de `BitBabbler`.
- `error.rs`: `BitBabblerError` e contexto das operações USB.
- `fold.rs`: `Fold`, conversão validada e XOR dos segmentos.
- `policy.rs`: timeouts, limites de sincronização, leitura e rejection sampling.
- `protocol.rs`: comandos FTDI/MPSSE, parsing de status e leitura exata.
- `transport.rs`: adaptação de enumeração, descritores e transferências `rusb`.

Uma trait privada representa as operações mínimas do transporte. Produção usa `rusb`; testes usam um transporte determinístico em memória. A trait não faz parte da API pública e não cria abstração para backends futuros.

## 15. Health checks e pureza da saída

A crate não porta `HealthMonitor`, ENT, FIPS ou qualquer limiar estatístico do pacote oficial. Não há gating da saída. Dados retornados são bytes lidos diretamente do hardware, com remoção apenas do framing/status FTDI e, quando solicitado explicitamente, folding por XOR.

A documentação deve evitar afirmar que a saída foi certificada, validada estatisticamente ou aprovada por health checks. Testes ou análises pertencem ao programa consumidor.

## 16. Testes

### 16.1 Sem hardware

- `Fold::default()` é `Raw`.
- `TryFrom<u8>` aceita 0 a 4 e rejeita outros valores antes de I/O.
- Vetores determinísticos validam folds 0, 1, 2, 3 e 4.
- Folding segmentado produz o mesmo resultado que folding sucessivo do bloco integral.
- `get_bits` valida 0 e tamanhos não alinhados antes de I/O.
- Tamanhos 8, 64, 72 e 8192 retornam o número exato de bytes.
- Falha após progresso parcial não expõe dados parciais.
- Parsing FTDI funciona com `wMaxPacketSize` 64 e 512.
- Status bytes divididos entre bulk reads são processados corretamente.
- Status inválido, excesso de dados, reads vazios e timeouts produzem o erro esperado.
- A sequência de inicialização MPSSE e seus parâmetros são verificados por transporte falso.
- Enumeração cobre zero, um e vários dispositivos.
- Seleção por serial cobre sucesso, ausência e serial vazio.
- Produto desconhecido é rejeitado.
- `random_range` cobre largura 1, potências de dois, larguras não potências, proximidade de `u64::MAX`, rejeição e esgotamento.
- `Drop` com transporte já desconectado não provoca panic.

### 16.2 Com hardware real

Testes de integração separados e explicitamente identificados como dependentes de hardware cobrem:

- enumeração e abertura de White;
- enumeração e abertura de Black;
- serial e variante detectados;
- `get_bits` raw;
- folds 1 a 4 com tamanho final exato;
- `random_u64` e `random_range`;
- erro após remoção física;
- Windows com WinUSB;
- Linux com regra udev/permissão adequada.

Os testes não exigem que duas amostras sejam diferentes e não usam limiares estatísticos frágeis.

### 16.3 Validação de projeto

O padrão de validação será:

```text
cargo fmt --all -- --check
cargo check --all-targets
cargo test --all-targets
cargo clippy --all-targets --all-features -- -D warnings
cargo test --doc
```

O projeto usará Rust edition 2024 e terá MSRV 1.85.0, como `intel_seed`. A implementação e `rusb` deverão passar os testes nesse toolchain antes do primeiro release. Builds e testes reais serão executados separadamente em Windows e Linux; cross-compilation isolada não substitui a validação com dispositivo em cada sistema.

## 17. Documentação operacional

O README deve documentar:

- instalação/associação manual do WinUSB no Windows;
- regra udev restrita a `0403:7840` no Linux;
- significado de raw e folds 1 a 4;
- redução de throughput por `2^fold`;
- ausência deliberada de health checks;
- comportamento com zero, um ou vários dispositivos;
- recuperação explícita após desconexão;
- integração síncrona e uso de `spawn_blocking` pelo consumidor.

## 18. Licenciamento

A nova crate será licenciada sob MIT. O código oficial em `bit-babbler-0.9` é GPL v2 e permanece apenas como referência de comportamento e protocolo. Nenhuma função ou estrutura C++ será copiada literalmente.

O diretório de referência será excluído do pacote Cargo publicado. A distribuição deverá respeitar também os termos aplicáveis do libusb, inclusive quando ele for vinculado estaticamente.

## 19. Critérios de aceitação

O primeiro release estará completo quando:

- a mesma API abrir e ler White e Black em Windows e Linux;
- `open()` aplicar a regra zero/um/vários dispositivos;
- seleção por serial funcionar;
- `get_bits` retornar raw por padrão;
- `get_bits_with_fold` aceitar somente folds 0 a 4 e retornar tamanho exato;
- `random_u64` e `random_range` permanecerem raw;
- nenhum health check ou transformação implícita for executado;
- falhas nunca expuserem buffers parciais;
- timeouts, desconexão e protocolo inválido produzirem erros tipados;
- testes sem hardware cobrirem folding, protocolo, seleção e sampling;
- testes reais passarem com pelo menos um White e um Black em cada plataforma disponível para validação;
- README explicar drivers, permissões, limitações e integração.
