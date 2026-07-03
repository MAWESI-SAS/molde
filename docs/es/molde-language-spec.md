# molde — especificación del lenguaje de modelos molde

> **Estado:** borrador para aprobación (Fase A, entregable 1).
> **Nombre provisional:** molde (*lenguaje de modelos molde*). Extensión de archivo: `.model`.
> **Objetivo:** reemplazar completamente C# como formato de definición de modelos dentro de molde. molde
> es **únicamente una capa de esquema**: administra y versiona modelos, genera
> migraciones, y realiza scaffolding. El acceso a datos en tiempo de ejecución está fuera de alcance. No hay
> generación de C# ni sidecar de .NET.
>
> **¿Prefieres aprender con ejemplos?** Esta es la referencia precisa; para un
> recorrido práctico basado en ejemplos, consulta
> [authoring-models.md](authoring-models.md).

---

## 1. Principios de diseño

1. **Legible para humanos y económico en tokens para agentes.** Sintaxis con
   sangría, estilo TOON/YAML, sin llaves ni puntuación superflua.
2. **Una entidad por archivo.** `Customer.model`, `Order.model`, … más un
   `database.model` para asuntos globales. Todo lo relativo a una tabla vive junto
   (columnas, PK, FKs, indexes, triggers, seed) — no existe un "DbContext" separado.
3. **El lenguaje es una proyección del IR.** Cada construcción se mapea 1:1 a un
   campo de `molde-core::model::DatabaseModel`. El IR sigue siendo el centro; `.model` es
   su forma textual canónica.
4. **Round-trip garantizado a nivel del IR.** `parse(emit(ir)) == ir`. Las
   construcciones de alto nivel (owned, inheritance, enums) son **azúcar sintáctico de
   autoría** que se expande a la forma relacional plana; el scaffolding desde la BD siempre
   emite la forma canónica (plana).
5. **Agnóstico de motor, con vías de escape.** Tipos lógicos que cada proveedor
   traduce; bloques `dbtype=` y `sql:` en bruto para asuntos específicos del motor.

---

## 2. Estructura del proyecto

```
models/
  database.model        # global metadata (schema, extensions, functions, raw)
  Customer.model        # one entity = one table
  Order.model
  Payment.model
migrations/             # sibling of models/, VISIBLE and versioned in git
  snapshot.json         # previous state (managed by molde, not hand-edited)
  20260607_*.json       # generated migrations
```

- Un archivo `<Entity>.model` define **exactamente una tabla**.
- El nombre del archivo es indicativo; el nombre real de la entidad/tabla se toma del contenido.
- `database.model` es opcional; si falta, se asumen los valores por defecto.

---

## 3. Estructura léxica

| Elemento | Sintaxis | Notas |
|---|---|---|
| Sangría | 2 espacios | define la estructura; sin tabs |
| Comentario | `# …` hasta el final de la línea | se descarta; no es el `comment` semántico |
| Identificador | `[A-Za-z_][A-Za-z0-9_]*` | nombres de entidad/columna/etc. |
| Escalar simple | `int`, `42`, `cascade` | sin comillas si no tiene espacios/caracteres especiales |
| String | `"text with spaces"` | comillas dobles; escapes `\"` `\\` |
| Nulo | `~` | representa `null` (p. ej. en seed) |
| Booleano | `true` / `false` | |
| Lista en línea | `[a, b, c]` | |
| Objeto en línea | `{k: v, k2: v2}` | |
| Lista en bloque | líneas con `- …` | |
| Bloque de texto | `|` + líneas con sangría | SQL multilínea (triggers, functions, raw, computed) |

Bloque de texto (escalar de bloque), idéntico en espíritu al `|` de YAML:

```
function: |
  CREATE OR REPLACE FUNCTION normalize_body() RETURNS trigger
  LANGUAGE plpgsql AS $$ BEGIN NEW.body := lower(NEW.body); RETURN NEW; END $$
```

---

## 4. Archivo de entidad

Forma general (todas las secciones excepto el encabezado y `fields:` son opcionales):

```
<Entity>[: <table>]            # header; ": <table>" only if it differs from Entity
  schema: <name>               # if it differs from the global default
  comment: "<text>"            # table COMMENT
  fields:
    <Col>: <type>[?] <facets…>
    …
  key: [<cols>] [name=<n>]     # composite PK or PK with explicit name
  belongs-to:                  # foreign keys (dependent side)
    <Nav>: {fk: …, references: …, onDelete: …, name: …}
  indexes:
    - <name>: {on: […], unique: …, method: …, operators: […], filter: …, expression: …}
  triggers:
    - <name>: {timing: …, events: […], function: …, sql: | …}
  seed:
    - {<Col>: <val>, …}
  # authoring sugar:
  owns <Type> { … }            # owned type → prefixed columns
  discriminator: <Col>         # TPH inheritance
  subtypes:
    <SubType>: { <extra fields> }
```

### 4.1 Encabezado

- `Customer` → la entidad y la tabla se llaman ambas `Customer`.
- `Customer: tbl_customer` → entidad `Customer`, tabla física `tbl_customer`.
- `schema:` sobrescribe el `schema` global por defecto para esta tabla.

---

## 5. Tipos lógicos

El **tipo lógico** reemplaza a `clr_type`. El proveedor deriva el `store_type`
por defecto a partir del tipo lógico + facetas. Mapeo canónico:

| Lógico | clr_type (IR) | store_type por defecto (ejemplo PG) |
|---|---|---|
| `int` | System.Int32 | integer |
| `long` | System.Int64 | bigint |
| `short` | System.Int16 | smallint |
| `byte` | System.Byte | smallint |
| `bool` | System.Boolean | boolean |
| `string` | System.String | text / varchar(n) si `maxlen=` |
| `decimal` | System.Decimal | numeric / numeric(p,s) si `precision=` |
| `double` | System.Double | double precision |
| `float` | System.Single | real |
| `datetime` | System.DateTime | timestamp |
| `datetimeoffset` | System.DateTimeOffset | timestamptz |
| `date` | System.DateOnly | date |
| `time` | System.TimeOnly | time |
| `guid` | System.Guid | uuid |
| `bytes` | System.Byte[] | bytea |
| `json` | System.String | jsonb (sobrescribir con `dbtype=`) |

**Tipo nativo en bruto.** Si el tipo no es un tipo lógico conocido, se interpreta
como un `store_type` literal y `clr_type` queda indeterminado:

```
Embedding: vector(1536)     # store_type = "vector(1536)"
Search:    tsvector         # store_type = "tsvector"
```

Equivalente con una sobrescritura sobre un tipo lógico:

```
Metadata: json dbtype=jsonb # clr System.String, store_type jsonb
```

Un `?` después del tipo marca la columna como **nullable**. Su ausencia = `NOT NULL`.
(No existe una faceta `required`: sería redundante con la ausencia de `?`.)

---

## 6. Facetas de columna

Después de `<Col>: <type>[?]`, una secuencia de facetas separadas por espacios.

**Booleano** (presencia = habilitado):

| Faceta | Campo IR |
|---|---|
| `pk` | agrega la columna a `primary_key` (PK simple; nombre por convención `pk_<table>`) |
| `identity` | `is_identity = true` |
| `unique` | crea un índice único de una sola columna (`ix_<table>_<col>`) |
| `stored` | `computed_stored = true` (usar junto con `computed=`) |

**Con un valor** (`key=value`):

| Faceta | Campo IR |
|---|---|
| `maxlen=<n>` | `max_length` |
| `precision=<p>[,<s>]` | `precision`, `scale` |
| `default=<sql>` | `default_value_sql` (SQL en bruto; usar comillas si contiene espacios) |
| `computed=<sql>` | `computed_sql` |
| `collation=<name>` | `collation` |
| `dbtype=<store_type>` | `store_type` (sobrescritura del tipo nativo exacto) |
| `clr=<type>` | `clr_type` (sobrescribe el tipo CLR mapeado; avanzado, rara vez necesario) |
| `comment="<text>"` | `comment` de la columna |
| `pk=<name>` | PK simple con nombre explícito |

Ejemplos:

```
Id:    int pk identity
Name:  string maxlen=200
Total: decimal precision=18,2
Flag:  bool default=false
Slug:  string computed="lower(name)" stored
Body:  json dbtype=jsonb comment="raw payload"
```

---

## 7. Clave primaria

- **Simple:** la faceta `pk` en la columna. Nombre por convención `pk_<table>`
  (todo en minúsculas), o explícito con `pk=<name>`.
- **Compuesta / nombrada a nivel de tabla:** el bloque `key:`.

```
key: [TenantId, Id]                 # name defaults to pk_<table>
key: [TenantId, Id] name=tenant_items_pk   # or an explicit custom name
```

Se mapea a `PrimaryKey { name, columns }`.

---

## 8. Relaciones (claves foráneas)

La FK vive en la tabla **dependiente**, bajo `belongs-to:`:

```
belongs-to:
  Customer: {fk: CustomerId, references: Customer.Id, onDelete: cascade}
  # name defaults to fk_<table>_<principal>; add `name: <custom>` to override
```

| Clave | Campo IR | Notas |
|---|---|---|
| (label) | — | nombre de navegación (documental) |
| `fk` | `columns` | columna(s) local(es); una lista si es compuesta: `[A, B]` |
| `references` | `principal_table` (+ `principal_schema`) y `principal_columns` | `Table.Col` o `schema.Table.[A,B]` |
| `onDelete` | `on_delete` | `no_action`\|`restrict`\|`cascade`\|`set_null`\|`set_default` |
| `name` | `name` | opcional; convención `fk_<table>_<principal>` (todo en minúsculas) |
| `index` | — | `false` excluye el índice de respaldo autogenerado (por defecto `true`) |

**Índice de respaldo (por convención).** Cada `belongs-to` obtiene automáticamente
un índice no único sobre su(s) columna(s) FK — nombrado `ix_<table>_<cols>`
(en minúsculas) — igual que en EF. No lo declaras, y está oculto en el `.model`
canónico (se resintetiza al parsear). Se **omite** cuando las columnas FK ya son
las columnas líderes de la primary key o de un índice existente, o cuando
estableces `index: false`. Una foreign key sin índice de cobertura se escribe de
vuelta como `index: false`, de modo que el estado siempre hace round-trip.

Las navegaciones **inversas** (lado principal) **no se modelan**: la FK ya está
descrita en el lado dependiente. Listarlas duplicaría información sin generar
DDL; si se necesitan diagramas/documentación, se derivan del conjunto de FKs.

---

## 9. Índices

Hay tres formas en que se crea un índice:

1. **Automático — foreign keys.** Cada `belongs-to` obtiene un índice de respaldo
   no único por convención (§8), nombrado `ix_<table>_<cols>`. No lo declaras y
   no se muestra en el `.model` canónico. Se excluye con `index: false`.
2. **En línea — único de una sola columna.** La faceta `unique` en una columna
   crea un índice único de una sola columna (`ix_<table>_<col>`).
3. **El bloque `indexes:` — todo lo demás** (estos los declaras tú): índices de
   performance sobre columnas regulares, índices compuestos, parciales/filtrados,
   métodos especiales (GIN/GiST/HNSW), índices de expresión.

> Regla práctica: relaciones → molde las indexa por ti; índices de
> consulta/performance → tú los declaras.

Cada entrada de `indexes:` es `- <name>: { <options> }`:

```
indexes:
  - ix_order_status_created: {on: [Status, CreatedAt]}          # composite, non-unique
  - ix_customer_email:       {on: [Email], unique: true}
  - ix_docs_fts:             {on: [], expression: "to_tsvector('english', body)", method: gin}
  - ix_emb:                  {on: [Embedding], method: hnsw, operators: [vector_cosine_ops]}
  - ix_active:               {on: [Status], filter: "Deleted = false"}
```

La etiqueta antes de `:` es el **nombre** del índice (tú lo eliges; en minúsculas
por convención). `on:` es la lista ordenada de columnas. El resto es opcional:

| Clave | Campo IR |
|---|---|
| (label) | `name` |
| `on` | `columns` |
| `unique` | `is_unique` |
| `filter` | `filter` (índice parcial) |
| `method` | `method` (`gin`, `gist`, `hnsw`, `ivfflat`, …) |
| `operators` | `operators` (clase de operador por columna) |
| `expression` | `expression` (índice funcional / full-text; tiene precedencia sobre `on`) |

---

## 10. Tipos owned (azúcar sintáctico)

```
fields:
  Contact: owns ContactInfo {Phone: string?, City: string? maxlen=80}
```

Se expande a columnas planas con el prefijo `<Prop>_<Field>`:
`Contact_Phone`, `Contact_City`. En el IR **solo existen las columnas planas**.
El scaffolding desde la BD emite las columnas planas (no reconstruye `owns`).

---

## 11. Herencia TPH (azúcar sintáctico)

```
Payment: Payment
  discriminator: Discriminator
  fields:
    Id:     int pk identity
    Amount: decimal
  subtypes:
    CardPayment: {CardNumber: string}
    CashPayment: {Note: string?}
```

Se expande a **una sola tabla** con: todas las columnas base + las de cada
subtipo (forzadas a `nullable`) + una columna discriminadora (`Discriminator
string`). En el IR es una tabla plana. El scaffolding desde la BD no reconstruye
`subtypes` (emite las columnas planas + la columna discriminadora como una
columna normal).

---

## 12. Convertidores de valor / enums (azúcar sintáctico)

```
Status: enum[Pending, Shipped] as=string maxlen=20
```

- `as=string` → columna `string` (store_type varchar); los valores válidos son
  documentación (molde no agrega un CHECK por defecto).
- `as=int` → columna `int`.

En el IR es una columna escalar normal. El scaffolding desde la BD ve la columna
escalar (no reconstruye el enum).

---

## 13. Datos seed

```
seed:
  - {Id: 1, Name: "ACME",   Email: "acme@example.com"}
  - {Id: 2, Name: "Globex", Email: ~}
```

Se mapea a `Table.seed_data` (una lista de mapas columna→valor-JSON). `~` = null.
El diff los materializa como `INSERT`/`UPDATE`/`DELETE`.

Las filas seed se emparejan entre migraciones por la **primary key** por
defecto. Para en su lugar emparejar por una **clave natural** — de modo que una
PK generada por la base de datos (p. ej. un `guid` con `gen_random_uuid()`)
pueda omitirse de las filas — decláralo con `seed-key:`:

```
seed-key: [Code]              # match rows by Code, not by the PK
seed:
  - {Code: "ACME", Name: "ACME Inc"}   # no Id; the DB generates it
```

Se mapea a `Table.seed_key` (una lista de nombres de columna; vacío ⇒ emparejar
por PK). Las columnas de `seed-key` deben ser únicas. `UpdateData` excluye las
columnas de la clave de emparejamiento.

---

## 14. Triggers (por entidad)

```
triggers:
  - trg_normalize:
      timing: before
      events: [insert, update]
      function: normalize_body
      sql: |
        CREATE TRIGGER trg_normalize BEFORE INSERT OR UPDATE ON public.documents
        FOR EACH ROW EXECUTE FUNCTION normalize_body()
```

| Clave | Campo IR (`Trigger`) |
|---|---|
| (label) | `name` |
| (entidad) | `table` (+ `schema`) |
| `timing` | `timing` (`before`\|`after`\|`instead_of`) |
| `events` | `events` (`insert`\|`update`\|`delete`\|`truncate`) |
| `function` | `function` (informativo) |
| `sql` | `definition` (DDL en bruto; fuente de verdad para recrearlo) |

---

## 15. Archivo global `database.model`

```
schema: public                 # default_schema
product-version: 9.0.0         # informational (optional)
extensions: [pg_trgm, unaccent, vector]
functions:
  - normalize_body: |
      CREATE OR REPLACE FUNCTION public.normalize_body() RETURNS trigger
      LANGUAGE plpgsql AS $$ BEGIN NEW.body := lower(NEW.body); RETURN NEW; END $$
raw:
  - |
      CREATE FULLTEXT CATALOG ft AS DEFAULT
```

| Clave | Campo IR (`DatabaseModel`) |
|---|---|
| `schema` | `default_schema` |
| `product-version` | `product_version` |
| `extensions` | `extensions` |
| `functions` | `functions` (lista de `DbFunction { name[, schema], definition }`) |
| `raw` | `raw_objects` (DDL textual) |

`format_version` es gestionado internamente por molde (no se edita).

---

## 16. Mapeo formal DSL ↔ IR (cobertura completa)

| IR | molde |
|---|---|
| `DatabaseModel.default_schema` | `database.model: schema:` |
| `DatabaseModel.product_version` | `database.model: product-version:` |
| `DatabaseModel.extensions` | `database.model: extensions:` |
| `DatabaseModel.functions` | `database.model: functions:` |
| `DatabaseModel.raw_objects` | `database.model: raw:` |
| `Table.name` / `.schema` | encabezado `<Entity>: <table>` / `schema:` |
| `Table.clr_type` | derivado (no se escribe; metadata) |
| `Table.comment` | `comment:` |
| `Table.columns` | `fields:` |
| `Table.primary_key` | faceta `pk` o bloque `key:` |
| `Table.foreign_keys` | `belongs-to:` |
| `Table.indexes` | faceta `unique` o bloque `indexes:` |
| `Table.triggers` | `triggers:` |
| `Table.seed_data` | `seed:` |
| `Table.seed_key` | `seed-key:` |
| `Column.name` | etiqueta del field |
| `Column.clr_type` | tipo lógico |
| `Column.store_type` | tipo nativo en bruto o `dbtype=` |
| `Column.is_nullable` | sufijo `?` |
| `Column.is_identity` | `identity` |
| `Column.max_length` | `maxlen=` |
| `Column.precision` / `.scale` | `precision=p,s` |
| `Column.default_value_sql` | `default=` |
| `Column.computed_sql` / `.computed_stored` | `computed=` / `stored` |
| `Column.collation` | `collation=` |
| `Column.comment` | `comment=` |
| `PrimaryKey.{name,columns}` | `pk` / `pk=` / `key:` |
| `ForeignKey.*` | `belongs-to:` (ver §8) |
| `Index.*` | `indexes:` (ver §9) |
| `Trigger.*` | `triggers:` (ver §14) |
| `DbFunction.*` | `functions:` (ver §15) |

**Sin huérfanos:** todo campo del IR tiene una representación. Los únicos que no
se escriben son `format_version` (interno) y el `clr_type` de la tabla (metadata
derivada).

---

## 17. Formas canónicas vs azúcar sintáctico

- **Canónica** = lo que produce el emisor (scaffold DB→`.model`): columnas
  planas, FKs explícitas, índices explícitos, sin `owns`/`subtypes`/`enum`.
- **Azúcar** = atajos de autoría humana (`owns`, `subtypes`, `enum[…]`) que se
  expanden al parsear. **No** sobreviven a una re-emisión.

Garantía: `parse(emit(ir)) == ir` para cualquier IR. `emit(parse(dsl))` produce
la forma canónica equivalente (puede diferir textualmente del azúcar original;
como un formatter).

---

## 18. Gramática (EBNF informal)

```ebnf
file        = entity_file | database_file ;

entity_file = header NEWLINE INDENT { section } DEDENT ;
header      = IDENT [ ":" ws name ] ;
section     = "schema:"  ws name
            | "comment:" ws string
            | "discriminator:" ws IDENT
            | fields_blk | key_line | belongs_blk
            | indexes_blk | triggers_blk | seed_blk
            | owns_inline | subtypes_blk ;

fields_blk  = "fields:" NEWLINE INDENT { field } DEDENT ;
field       = IDENT ":" ws type [ "?" ] { ws facet } NEWLINE ;
type        = logical_type | native_type | enum_type | owns_type ;
enum_type   = "enum[" idlist "]" ;
owns_type   = "owns" ws IDENT ws inline_obj ;
facet       = "pk" | "identity" | "unique" | "stored"
            | "maxlen=" INT | "precision=" INT [ "," INT ]
            | "default=" value | "computed=" value | "collation=" name
            | "column=" name | "dbtype=" name | "comment=" string
            | "pk=" name | "as=" ("string"|"int") ;

key_line    = "key:" ws list [ ws "name=" name ] ;
belongs_blk = "belongs-to:" NEWLINE INDENT { fk_entry } DEDENT ;
fk_entry    = IDENT ":" ws inline_obj ;       (* keys: fk, references, onDelete, name *)

indexes_blk = "indexes:" NEWLINE INDENT { "-" ws IDENT ":" ws inline_obj } DEDENT ;
triggers_blk= "triggers:" NEWLINE INDENT { trigger } DEDENT ;
trigger     = "-" ws IDENT ":" ( inline_obj | NEWLINE INDENT { kv } DEDENT ) ;

seed_blk    = "seed:" NEWLINE INDENT { "-" ws inline_obj } DEDENT ;

database_file = { db_section } ;
db_section  = "schema:" ws name | "product-version:" ws name
            | "extensions:" ws list
            | "functions:" NEWLINE INDENT { "-" ws IDENT ":" ws block } DEDENT
            | "raw:" NEWLINE INDENT { "-" ws block } DEDENT ;

value       = scalar | string | list | inline_obj | block ;
list        = "[" [ value { "," value } ] "]" ;
inline_obj  = "{" [ kv { "," kv } ] "}" ;
kv          = IDENT ":" ws value ;
block       = "|" NEWLINE INDENT { TEXT } DEDENT ;
```

---

## 19. Ejemplo completo (ver `examples/models/`)

`database.model`
```
schema: public
```

`Customer.model`
```
Customer
  fields:
    Id:      int pk
    Name:    string maxlen=200
    Email:   string? unique maxlen=320
    Contact: owns ContactInfo {Phone: string?}
  seed:
    - {Id: 1, Name: "ACME",   Email: "acme@example.com"}
    - {Id: 2, Name: "Globex", Email: ~}
```

`Order.model`
```
Order
  fields:
    Id:         int pk identity
    CustomerId: int
    Total:      decimal precision=18,2
    Status:     enum[Pending, Shipped] as=string maxlen=20
  belongs-to:
    Customer: {fk: CustomerId, references: Customer.Id, onDelete: cascade, name: fk_order_customer}
```

`Payment.model`
```
Payment
  discriminator: Discriminator
  fields:
    Id:     int pk identity
    Amount: decimal
  subtypes:
    CardPayment: {CardNumber: string}
    CashPayment: {Note: string?}
```

Forma **canónica** de `Customer.model` después de una re-emisión (azúcar expandido):
```
Customer
  fields:
    Id:            int pk
    Name:          string maxlen=200
    Email:         string? maxlen=320
    Contact_Phone: string?
  indexes:
    - ix_customer_email: {on: [Email], unique: true}
  seed:
    - {Id: 1, Name: "ACME",   Email: "acme@example.com"}
    - {Id: 2, Name: "Globex", Email: ~}
```

---

## 20. Decisiones (resueltas)

1. **Nombre y extensión:** lenguaje molde, extensión **`.model`**. ✔
2. **`?` como único marcador de nullability** (sin `required`). ✔
3. **Navegaciones inversas: NO modeladas** (ver §8). Evita duplicación; sin
   efecto en el DDL. ✔
4. **Snapshot y migraciones en `migrations/`** (visible, hermano de `models/`,
   versionado en git; `migrations/snapshot.json`). ✔
5. **Enums sin CHECK** por defecto: `enum` solo establece el tipo de columna
   (alineado con EF y con un round-trip limpio). Una faceta `check` queda como una
   extensión futura opcional. ✔
