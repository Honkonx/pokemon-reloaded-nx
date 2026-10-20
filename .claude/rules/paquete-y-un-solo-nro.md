# Un solo NRO y un paquete que se genera

- El producto se instala con **un solo `.nro`** o con una carpeta que se copia a la microSD. Cualquier componente auxiliar va **incrustado** en el NRO principal; no se entregan varios ejecutables.
- La carpeta de datos canónica es `sdmc:/switch/pokenx/` (juego, guardados, registros, ajustes). Copiar esa carpeta o el NRO es toda la instalación.
- `pokenx/` **no se versiona**: la crea `app/build-package.ps1` (NRO, carpetas, `README.txt`, licencias de Mesa y, si se indica, una SoundFont con su licencia) y el script exige que haya exactamente un `.nro`.
- Tras cualquier cambio que afecte al NRO (menú, motor, Mesa, ajustes), regenera el paquete y anota el **tamaño y el SHA-256** del `pokenx/pokenx.nro`, y qué cambió respecto al anterior. Si el cambio no toca el NRO, dilo.
- `app/build-package.ps1` se niega a empaquetar si `pokenx/game` contiene archivos del juego; `scripts/make-release.ps1` también.
