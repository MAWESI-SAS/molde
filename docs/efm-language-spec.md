# EFM — Especificación del lenguaje de modelos de efrust

> **Estado:** borrador para aprobación (Fase A, entregable 1).
> **Nombre provisional:** EFM (*efrust model language*). Extensión de archivo: `.model`.
> **Objetivo:** reemplazar por completo a C# como formato de definición de modelos
> dentro de efrust. efrust es **solo capa de esquema**: gestiona y versiona modelos,
> genera migraciones y hace scaffold. El acceso a datos en runtime queda fuera de
> alcance. No hay generación de C# ni sidecar .NET.

---

## 1. Principios de diseño

1. **Legible por humanos y barato en tokens para agentes.** Sintaxis indentada
   estilo TOON/YAML, sin llaves ni puntuación superflua.
2. **Una entidad por archivo.** `Customer.model`, `Order.model`, … más un
   `database.model` para lo global. Todo lo de una tabla vive junto (columnas,
   PK, FKs, índices, triggers, seed) — no hay un "DbContext" aparte.
3. **El lenguaje es una proyección del IR.** Cada construcción mapea 1:1 a un
   campo de `efrust-core::model::DatabaseModel`. El IR sigue siendo el centro;
   `.model` es su forma textual canónica.
4. **Round-trip garantizado a nivel IR.** `parse(emit(ir)) == ir`. Las
   construcciones de alto nivel (owned, herencia, enums) son **azúcar de
   escritura** que se expande a la forma relacional plana; el scaffold desde BD
   emite siempre la forma canónica (plana).
5. **Agnóstico del motor, con escape hatches.** Tipos lógicos que cada provider
   traduce; `dbtype=` y bloques `sql:` crudos para lo específico de un motor.

---

## 2. Estructura del proyecto

```
models/
  database.model        # metadatos globales (schema, extensiones, funciones, raw)
  Customer.model        # una entidad = una tabla
  Order.model
  Payment.model
migrations/             # hermano de models/, VISIBLE y versionado en git
  snapshot.json         # estado previo (gestionado por efrust, no se edita a mano)
  20260607_*.json       # migraciones generadas
```

- Un archivo `<Entity>.model` define **exactamente una tabla**.
- El nombre del archivo es indicativo; el nombre real de la entidad/tabla se
  toma del contenido.
- `database.model` es opcional; si falta, se asumen valores por defecto.

---

## 3. Léxico

| Elemento | Sintaxis | Notas |
|---|---|---|
| Indentación | 2 espacios | define la estructura; sin tabs |
| Comentario | `# …` hasta fin de línea | se descarta; no es el `comment` semántico |
| Identificador | `[A-Za-z_][A-Za-z0-9_]*` | nombres de entidad/columna/etc. |
| Escalar simple | `int`, `42`, `cascade` | sin comillas si no hay espacios/especiales |
| Cadena | `"texto con espacios"` | comillas dobles; escapes `\"` `\\` |
| Nulo | `~` | representa `null` (p. ej. en seed) |
| Booleano | `true` / `false` | |
| Lista inline | `[a, b, c]` | |
| Objeto inline | `{k: v, k2: v2}` | |
| Lista en bloque | líneas con `- …` | |
| Bloque de texto | `|` + líneas indentadas | SQL multilínea (triggers, funciones, raw, computed) |

Bloque de texto (block scalar), idéntico en espíritu a YAML `|`:

```
function: |
  CREATE OR REPLACE FUNCTION normalize_body() RETURNS trigger
  LANGUAGE plpgsql AS $$ BEGIN NEW.body := lower(NEW.body); RETURN NEW; END $$
```

---

## 4. Archivo de entidad

Forma general (todas las secciones salvo la cabecera y `fields:` son opcionales):

```
<Entity>[: <table>]            # cabecera; ": <table>" solo si difiere del Entity
  schema: <name>               # si difiere del default global
  comment: "<texto>"           # COMMENT de la tabla
  fields:
    <Col>: <type>[?] <facetas…>
    …
  key: [<cols>] [name=<n>]     # PK compuesta o con nombre explícito
  belongs-to:                  # claves foráneas (lado dependiente)
    <Nav>: {fk: …, references: …, onDelete: …, name: …}
  indexes:
    - <name>: {on: […], unique: …, method: …, operators: […], filter: …, expression: …}
  triggers:
    - <name>: {timing: …, events: […], function: …, sql: | …}
  seed:
    - {<Col>: <val>, …}
  # azúcar de escritura:
  owns <Type> { … }            # owned type → columnas con prefijo
  discriminator: <Col>         # herencia TPH
  subtypes:
    <SubType>: { <campos extra> }
```

### 4.1 Cabecera

- `Customer` → entidad y tabla se llaman `Customer`.
- `Customer: tbl_customer` → entidad `Customer`, tabla física `tbl_customer`.
- `schema:` sobreescribe el `schema` global por defecto para esta tabla.

---

## 5. Tipos lógicos

El **tipo lógico** reemplaza a `clr_type`. El provider deriva el `store_type`
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
| `json` | System.String | jsonb (override con `dbtype=`) |

**Tipo nativo crudo.** Si el tipo no es un lógico conocido, se interpreta como
`store_type` literal y `clr_type` queda sin determinar:

```
Embedding: vector(1536)     # store_type = "vector(1536)"
Search:    tsvector         # store_type = "tsvector"
```

Equivalente con override sobre un lógico:

```
Metadata: json dbtype=jsonb # clr System.String, store_type jsonb
```

`?` tras el tipo marca la columna como **nullable**. Su ausencia = `NOT NULL`.
(No existe faceta `required`: sería redundante con la ausencia de `?`.)

---

## 6. Facetas de columna

Tras `<Col>: <type>[?]`, una secuencia de facetas separadas por espacios.

**Booleanas** (presencia = activado):

| Faceta | Campo IR |
|---|---|
| `pk` | añade la columna a `primary_key` (PK simple; nombre por convención `PK_<table>`) |
| `identity` | `is_identity = true` |
| `unique` | crea un índice único de 1 columna (`IX_<table>_<col>`) |
| `stored` | `computed_stored = true` (usar junto a `computed=`) |

**Con valor** (`clave=valor`):

| Faceta | Campo IR |
|---|---|
| `maxlen=<n>` | `max_length` |
| `precision=<p>[,<s>]` | `precision`, `scale` |
| `default=<sql>` | `default_value_sql` (SQL crudo; usar comillas si tiene espacios) |
| `computed=<sql>` | `computed_sql` |
| `collation=<name>` | `collation` |
| `column=<db_name>` | nombre físico si difiere del nombre de la propiedad |
| `dbtype=<store_type>` | `store_type` (override del tipo nativo exacto) |
| `comment="<texto>"` | `comment` de la columna |
| `pk=<name>` | PK simple con nombre explícito |

Ejemplos:

```
Id:    int pk identity
Name:  string maxlen=200
Total: decimal precision=18,2
Flag:  bool default=false
Slug:  string computed="lower(name)" stored
Body:  json dbtype=jsonb comment="payload crudo"
```

---

## 7. Clave primaria

- **Simple:** faceta `pk` en la columna. Nombre por convención `PK_<table>`,
  o explícito con `pk=<name>`.
- **Compuesta / con nombre a nivel tabla:** bloque `key:`.

```
key: [TenantId, Id] name=PK_Item
```

Mapea a `PrimaryKey { name, columns }`.

---

## 8. Relaciones (claves foráneas)

La FK vive en la tabla **dependiente**, bajo `belongs-to:`:

```
belongs-to:
  Customer: {fk: CustomerId, references: Customer.Id, onDelete: cascade, name: FK_Order_Customer}
```

| Clave | Campo IR | Notas |
|---|---|---|
| (etiqueta) | — | nombre de navegación (documental) |
| `fk` | `columns` | columna(s) local(es); lista si compuesta: `[A, B]` |
| `references` | `principal_table` (+ `principal_schema`) y `principal_columns` | `Tabla.Col` o `esquema.Tabla.[A,B]` |
| `onDelete` | `on_delete` | `no_action`\|`restrict`\|`cascade`\|`set_null`\|`set_default` |
| `name` | `name` | opcional; convención `FK_<tabla>_<principal>` |

Las navegaciones **inversas** (lado principal) **no se modelan**: la FK ya queda
descrita en el lado dependiente. Listarlas duplicaría información sin generar
DDL; si se necesitan diagramas/documentación se derivan del conjunto de FKs.

---

## 9. Índices

- **Inline:** faceta `unique` en una columna (índice único de 1 columna).
- **Bloque `indexes:`** para todo lo demás:

```
indexes:
  - IX_Customer_Email: {on: [Email], unique: true}
  - ix_docs_fts:       {on: [], expression: "to_tsvector('english', body)", method: gin}
  - ix_emb:            {on: [Embedding], method: hnsw, operators: [vector_cosine_ops]}
  - ix_active:         {on: [Status], filter: "Deleted = false"}
```

| Clave | Campo IR |
|---|---|
| (etiqueta) | `name` |
| `on` | `columns` |
| `unique` | `is_unique` |
| `filter` | `filter` (índice parcial) |
| `method` | `method` (`gin`, `gist`, `hnsw`, `ivfflat`, …) |
| `operators` | `operators` (operator class por columna) |
| `expression` | `expression` (índice funcional / full-text; manda sobre `on`) |

---

## 10. Owned types (azúcar)

```
fields:
  Contact: owns ContactInfo {Phone: string?, City: string? maxlen=80}
```

Se expande a columnas planas con prefijo `<Prop>_<Campo>`:
`Contact_Phone`, `Contact_City`. En el IR **solo existen las columnas planas**.
El scaffold desde BD emite las columnas planas (no reconstruye `owns`).

---

## 11. Herencia TPH (azúcar)

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
subtipo (forzadas a `nullable`) + una columna discriminadora
(`Discriminator string`). En el IR es una tabla plana. El scaffold desde BD no
reconstruye `subtypes` (emite las columnas planas + la columna discriminadora
como una columna normal).

---

## 12. Value converters / enums (azúcar)

```
Status: enum[Pending, Shipped] as=string maxlen=20
```

- `as=string` → columna `string` (store_type varchar); los valores válidos son
  documentación (efrust no añade CHECK por defecto).
- `as=int` → columna `int`.

En el IR es una columna escalar normal. El scaffold desde BD ve la columna
escalar (no reconstruye el enum).

---

## 13. Seed data

```
seed:
  - {Id: 1, Name: "ACME",   Email: "acme@example.com"}
  - {Id: 2, Name: "Globex", Email: ~}
```

Mapea a `Table.seed_data` (lista de mapas columna→valor JSON). `~` = null.
El diff las materializa como `INSERT`/`UPDATE`/`DELETE`.

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
| (etiqueta) | `name` |
| (entidad) | `table` (+ `schema`) |
| `timing` | `timing` (`before`\|`after`\|`instead_of`) |
| `events` | `events` (`insert`\|`update`\|`delete`\|`truncate`) |
| `function` | `function` (informativo) |
| `sql` | `definition` (DDL crudo; fuente de verdad para recrearlo) |

---

## 15. Archivo global `database.model`

```
schema: public                 # default_schema
product-version: 9.0.0         # informativo (opcional)
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
| `raw` | `raw_objects` (DDL verbatim) |

`format_version` lo gestiona efrust internamente (no se edita).

---

## 16. Mapeo formal DSL ↔ IR (cobertura completa)

| IR | EFM |
|---|---|
| `DatabaseModel.default_schema` | `database.model: schema:` |
| `DatabaseModel.product_version` | `database.model: product-version:` |
| `DatabaseModel.extensions` | `database.model: extensions:` |
| `DatabaseModel.functions` | `database.model: functions:` |
| `DatabaseModel.raw_objects` | `database.model: raw:` |
| `Table.name` / `.schema` | cabecera `<Entity>: <table>` / `schema:` |
| `Table.clr_type` | derivado (no se escribe; metadato) |
| `Table.comment` | `comment:` |
| `Table.columns` | `fields:` |
| `Table.primary_key` | faceta `pk` o bloque `key:` |
| `Table.foreign_keys` | `belongs-to:` |
| `Table.indexes` | faceta `unique` o bloque `indexes:` |
| `Table.triggers` | `triggers:` |
| `Table.seed_data` | `seed:` |
| `Column.name` | etiqueta del campo (físico: `column=`) |
| `Column.clr_type` | tipo lógico |
| `Column.store_type` | tipo nativo crudo o `dbtype=` |
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

**Sin huérfanos:** todos los campos del IR tienen representación. Los únicos no
escritos son `format_version` (interno) y `clr_type` de tabla (metadato derivado).

---

## 17. Formas canónicas vs azúcar

- **Canónico** = lo que produce el emitter (scaffold BD→`.model`): columnas
  planas, FKs explícitas, índices explícitos, sin `owns`/`subtypes`/`enum`.
- **Azúcar** = atajos de escritura humana (`owns`, `subtypes`, `enum[…]`,
  `has-many`) que se expanden al parsear. **No** sobreviven a un re-emit.

Garantía: `parse(emit(ir)) == ir` para todo IR. `emit(parse(dsl))` produce la
forma canónica equivalente (puede diferir textualmente del azúcar original;
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

## 19. Ejemplo completo (SampleModel)

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
    Customer: {fk: CustomerId, references: Customer.Id, onDelete: cascade, name: FK_Order_Customer}
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

Forma **canónica** de `Customer.model` tras un re-emit (azúcar expandida):
```
Customer
  fields:
    Id:            int pk
    Name:          string maxlen=200
    Email:         string? maxlen=320
    Contact_Phone: string?
  indexes:
    - IX_Customer_Email: {on: [Email], unique: true}
  seed:
    - {Id: 1, Name: "ACME",   Email: "acme@example.com"}
    - {Id: 2, Name: "Globex", Email: ~}
```

---

## 20. Decisiones (resueltas)

1. **Nombre y extensión:** lenguaje EFM, extensión **`.model`**. ✔
2. **`?` como única marca de nullabilidad** (sin `required`). ✔
3. **Navegaciones inversas: NO se modelan** (ver §8). Se evita duplicación; sin
   efecto en el DDL. ✔
4. **Snapshot y migraciones en `migrations/`** (visible, hermano de `models/`,
   versionado en git; `migrations/snapshot.json`). ✔
5. **Enums sin CHECK** por defecto: `enum` solo fija el tipo de columna (alineado
   con EF y con un round-trip limpio). Faceta `check` queda como extensión
   futura opcional. ✔
```
