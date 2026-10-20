---
name: rust-switch-nro-build
description: Compilar y empaquetar el NRO del proyecto (motor Rust + C++/libnx + Mesa) y comprobar cada etapa; úsala cuando se pida un build o su verificación.
---

# Compilar y empaquetar el NRO

1. **Requisitos:** `REQUISITOS.md` y `docs/GUIA.md`. Rust `nightly-2026-09-29` con `rust-src`; devkitPro con `devkitA64`, `libnx` y `switch-tools`; el SDK de Mesa en `target/mesa26/sdk`.
2. `pwsh app/build-package.ps1` compila el motor (`cargo build --release`, biblioteca estática `no_std` para `aarch64-nintendo-switch-freestanding` con `-Z build-std`), compila y enlaza el C++ con Mesa y libnx, **verifica las relocaciones**, crea el NRO y arma `pokenx/`.
3. Mantén distintos los errores de **compilación**, de **enlace** y de **empaquetado**. Revisa el tipo de archivo (ELF AArch64), los metadatos, el icono y el tamaño. Generar el NRO no prueba que arranque.
4. No versiones el NRO, el ELF, las salidas de `target/` ni el juego.
5. Informa cada etapa por separado: código compila, NRO generado, copiado y abierto en la consola, arranque visible, y funciones probadas en hardware.
6. No instales herramientas ni despliegues a una consola sin que quien mantiene el proyecto lo autorice.
