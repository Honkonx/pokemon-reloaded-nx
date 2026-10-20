# Cómo contribuir

Gracias por querer ayudar. Esta guía resume lo que se espera de un cambio. Si trabajas con un asistente de IA, dale también [AGENTS.md](AGENTS.md).

## Antes de empezar

1. Lee el [README](README.md) y la [guía de compilación](docs/GUIA.md). `pwsh scripts/doctor.ps1` te dice qué falta en tu equipo.
2. Para cambios grandes, abre antes una incidencia y cuenta qué quieres hacer: así evitamos trabajo duplicado.
3. Usa siempre el toolchain fijado, `nightly-2026-09-29`.

## Qué debe cumplir un cambio

- **Ni el juego ni sus datos** en el repositorio, en incidencias, registros o capturas. Las pruebas automáticas usan datos sintéticos o libres.
- **Sin rutas ni datos de tu equipo** (carpeta personal, nombre de usuario, correos, tokens). Usa rutas relativas o parámetros.
- **Pruebas:** el código nuevo lleva su prueba. La lógica que no depende de la consola (formatos, parsers, copias de archivos, mapeo de botones) se prueba en el PC.
- **Un solo NRO** como entregable. `pokenx/` se genera con `app/build-package.ps1`; no se versiona.
- **Arranque del NRO:** nunca enlaces con `-z notext` sin la comprobación de relocaciones (`scripts/verify_relocations.py`) y no silencies `read-only segment has dynamic relocations`. Detalle en [SISTEMA.md](docs/SISTEMA.md#arranque-seguro-del-nro).
- **Menú gráfico:** sin `consoleInit`/`printf` mientras está activo; cada pantalla nueva se comprueba con la vista previa (`tools/menu_preview`).
- **Código claro:** la solución más simple que cumpla el comportamiento. Imita el estilo del código que tocas (nombres, comentarios, idioma).

## Antes de enviar

```powershell
pwsh scripts/check.ps1        # pruebas del PC y compilación cruzada del motor
pwsh app/build-package.ps1    # si tocaste el NRO: anota el tamaño y el SHA-256
```

La integración continua ejecuta `scripts/check.ps1` en cada envío. **Compilar no es «funciona»:** en tu descripción di hasta qué etapa llegaste (compila en el PC, compila para la Switch, NRO generado, abre, renderiza, suena, mandos, se juega) y cuál no probaste. Las pruebas en una consola las hace quien tenga una; nadie está obligado a tenerla para contribuir.

## Incidencias

Usa la plantilla. Los registros (`boot.log`, `engine.log`, `crash.log`, `informe.txt`) no contienen datos del juego y son lo que más ayuda.

## Mensajes de commit

Una primera línea clara en imperativo (qué cambia), y si hace falta un párrafo con el porqué. Sin rutas locales ni nombres de equipo.
