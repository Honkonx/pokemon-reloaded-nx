# Sin rutas locales ni datos personales en el repositorio

Nada de lo que se commitee (código, comentarios, scripts, documentos, `.claude/`, mensajes de commit, nombres de ramas y etiquetas) debe mencionar:

- rutas de tu máquina (la carpeta personal de Windows, el escritorio, la carpeta de datos de aplicaciones), el nombre del equipo o de tu cuenta;
- correos, tokens, claves ni cuentas;
- datos del juego, rutas a tu copia ni nombres internos de sus scripts.

Cómo se escribe en su lugar: rutas **relativas** a la raíz del repositorio o parámetros (`-DevkitPro <ruta>`); ubicaciones estándar de un instalador solo como valor por defecto sustituible; ejemplos con marcadores (`<ruta-del-juego>`, `<tu-sd>`). Antes de un `push`, revisa `git diff --stat` y `git log --format=%B`.
