# Lunu Builder (C++)

Este diretório contém a estrutura base para o **Sistema de Build do Lunu**, escrito em C++.
O objetivo deste projeto é permitir a compilação e empacotamento de scripts `.luau` em executáveis ou pacotes otimizados.

## Estrutura do Projeto

builder/
├── CMakeLists.txt      # Configuração de Build (CMake)
├── src/                # Código Fonte (.cpp)
│   └── main.cpp        # Ponto de entrada CLI
├── include/            # Cabeçalhos (.h)
└── tests/              # Testes Unitários

## Como Começar (Para o Desenvolvedor C++)

### Pré-requisitos
- Compilador C++ (MSVC, GCC ou Clang) com suporte a C++20.
- CMake 3.20+.

### Compilando

1. Crie uma pasta de build:
   bash
   mkdir build
   cd build

2. Gere os arquivos de projeto:
   bash
   cmake ..

3. Compile:
   bash
   cmake --build . --config Release

O executável será gerado em `build/bin/lunu-builder.exe`.

## Objetivos do Builder

O amigo desenvolvedor deve focar em:
1.  **Parsing**: Ler arquivos `.luau`.
2.  **Bytecode**: Talvez integrar com `luau-compile` (C++) para gerar bytecode.
3.  **Packaging**: Empacotar o bytecode junto com o runtime do Lunu.
4.  **Native**: Criar hooks nativos para bibliotecas C++.

## Especificação de Build (Requisitos do Cliente)

O sistema deve funcionar de forma similar ao **PyInstaller** ou builds de Go/Rust:

1.  **Input**: O usuário fornece o script principal (ex: `lunu build main.luau`).
2.  **Output**: O builder gera um executável standalone (ex: `main.exe`).
3.  **Localização**: O executável gerado **DEVE** aparecer no diretório raiz onde o comando foi executado (Current Working Directory), e não escondido em subpastas de build.
4.  **Estilo**: Single-file executable (se possível), contendo o runtime e os scripts embutidos.

## Exemplo de Uso

```bash
# Comando
./lunu-builder build main.luau

# Resultado Esperado
# Criação do arquivo 'main.exe' na pasta atual.
```
