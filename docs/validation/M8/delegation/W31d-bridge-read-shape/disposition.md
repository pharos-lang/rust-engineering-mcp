# W31d — disposición del orquestador (2026-09-15)

Veredicto: **aceptado**. `readResource` devuelve `{ result, uri, … }` (bundle `index.js`): la aserción lee `result.contents` y exige `uri` + `blob|text` por contenido; cancel/EOF ya usaban la forma correcta. Sesión `runtime` real ejecutada por el worker: exit 0, 15 s. Unit tests verdes.
