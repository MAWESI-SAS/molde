# Bloqueo de pull requests con `molde ci`

`molde ci` ejecuta las verificaciones que deben bloquear un merge e imprime un
único reporte en Markdown (consulta la [referencia de CLI](cli.md#molde-ci)):

1. **lint** — análisis estático de seguridad de las migraciones (sin base de
   datos). Falla ante cambios destructivos; con `--strict`, también ante
   advertencias.
2. **snapshot** — `snapshot.json` debe estar actualizado respecto a los
   modelos.
3. **verify** — *opcional*: con `--connection`, aplica cada migración a una
   base de datos efímera desde cero y confirma que no haya drift respecto a
   los modelos. Se omite cuando no se indica una conexión.

Termina con código distinto de cero si alguna verificación falla. Ejecútalo
localmente cuando quieras:

```bash
molde ci                                            # lint + snapshot (no DB)
molde ci --connection "postgres://…" --report ci.md # + verify, and save the report
```

## Un workflow de GitHub Actions listo para copiar

Coloca esto en tu proyecto como `.github/workflows/molde-ci.yml`. Este
workflow levanta un PostgreSQL descartable, ejecuta `molde ci`, publica el
reporte como un comentario fijo (sticky) en el PR, y falla el job (bloqueando
el merge) si alguna verificación falló.

```yaml
name: molde

on:
  pull_request:

permissions:
  contents: read
  pull-requests: write   # to post the report comment

jobs:
  molde-ci:
    runs-on: ubuntu-latest
    services:
      postgres:
        image: postgres:16
        env:
          POSTGRES_PASSWORD: postgres
          POSTGRES_DB: molde_ci
        ports: ["5432:5432"]
        options: >-
          --health-cmd "pg_isready -U postgres"
          --health-interval 5s --health-timeout 5s --health-retries 10
    steps:
      - uses: actions/checkout@v4

      - name: Install molde
        run: |
          curl -L -o molde.tar.gz \
            https://github.com/MAWESI-SAS/molde/releases/latest/download/molde-v0.0.5-x86_64-unknown-linux-musl.tar.gz
          tar xzf molde.tar.gz
          sudo install molde /usr/local/bin/molde
          molde --version || molde --help | head -1

      - name: Run molde ci
        id: molde_ci
        continue-on-error: true
        env:
          DATABASE_URL: postgres://postgres:postgres@localhost:5432/molde_ci
        run: molde ci --connection "$DATABASE_URL" --report molde-ci.md

      - name: Post report as a PR comment
        if: always()
        uses: marocchino/sticky-pull-request-comment@v2
        with:
          path: molde-ci.md

      - name: Enforce the merge gate
        if: steps.molde_ci.outcome == 'failure'
        run: exit 1
```

### Cómo funciona

- El **service** `postgres` le da a `molde ci` una base de datos vacía;
  `verify` aplica cada migración en ella desde cero, así que una ejecución
  limpia demuestra que las migraciones construyen el esquema que describen
  tus modelos.
- `continue-on-error: true` permite que el workflow **siempre publique el
  comentario** (incluso si falla); el paso final vuelve a fallar el job para
  que la verificación siga siendo obligatoria.
- `marocchino/sticky-pull-request-comment` crea el comentario una vez y lo
  actualiza en el mismo lugar en pushes posteriores. ¿Prefieres no usar una
  action de terceros? Publica `molde-ci.md` con `actions/github-script` en su
  lugar.

### Notas y personalización

- **Fija la versión.** El ejemplo descarga `v0.0.5`; actualízala (o compílala
  desde el código fuente con `cargo install`) a medida que actualices. Otros
  targets precompilados están en la [página de releases](https://github.com/MAWESI-SAS/molde/releases/latest).
- **Acota el lint al PR.** Por defecto se lintea cada migración. Para revisar
  solo lo que agrega el PR, pasa `--since <base-migration-id>` (el id de la
  última migración en tu rama destino).
- **Modo estricto.** Agrega `--strict` para fallar también ante *advertencias*
  de lint (por ejemplo, agregar un índice único o una foreign key que podría
  fallar con datos existentes).
- **¿Sin base de datos?** Quita el service y el flag `--connection`; `molde ci`
  entonces ejecuta solo lint + snapshot, y la verificación verify se reporta
  como omitida (skipped).
- **Otros motores.** Apunta `--connection` a un service de MySQL o SQL Server;
  define `--provider` si no se puede inferir de la URL.

> `molde init-team --ci github` genera un workflow inicial con el merge
> driver de snapshot para equipos; esta página es la versión más completa,
> que comenta en el PR, construida alrededor de `molde ci`.
```
