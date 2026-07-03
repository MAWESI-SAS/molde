# Cómo escribir modelos — una guía práctica

Esta guía te enseña a escribir archivos `.model` con ejemplos: cómo definir
tablas, cada característica que puedes usar, y cómo llevar un modelo hasta una
base de datos real. Es el complemento práctico de la
[especificación del lenguaje](molde-language-spec.md) precisa y de la
[referencia de CLI](cli.md).

> Un archivo `.model` describe el **esquema** de una tabla. molde convierte estos
> archivos en migraciones y las aplica a PostgreSQL, MySQL, SQLite o SQL Server.

## Contenido

1. [El panorama general](#1-the-big-picture)
2. [Tu primer modelo](#2-your-first-model)
3. [Campos y tipos](#3-fields-and-types)
4. [Nulabilidad](#4-nullability)
5. [Llaves primarias](#5-primary-keys)
6. [Identity / auto-incremento](#6-identity--auto-increment)
7. [Facets de columna (defaults, longitudes, precisión, comentarios…)](#7-column-facets)
8. [Columnas calculadas](#8-computed-columns)
9. [Restricciones unique](#9-unique-constraints)
10. [Relaciones (foreign keys)](#10-relationships-foreign-keys)
11. [Índices](#11-indexes)
12. [Datos seed](#12-seed-data)
13. [Triggers](#13-triggers)
14. [El archivo global `database.model`](#14-the-global-databasemodel-file)
15. [Atajos de autoría: owned types, enums, herencia](#15-authoring-shortcuts)
16. [Tipos específicos de motor y escapes](#16-engine-specific-types-and-escape-hatches)
17. [El flujo de trabajo del día a día](#17-the-everyday-workflow)
18. [Un ejemplo completo resuelto](#18-a-complete-worked-example)

---

## 1. El panorama general

- **Una entidad por archivo.** `Customer.model` describe la tabla `Customer`,
  `Order.model` la tabla `Order`, y así sucesivamente. Todos viven en un
  directorio `models/`.
- **Un archivo global.** Un `database.model` opcional contiene la configuración
  compartida por toda la base de datos (esquema por defecto, extensiones,
  funciones, DDL crudo).
- **La indentación importa.** La estructura se define con indentación de
  **2 espacios** — sin tabs, sin llaves para los bloques.
- **Dos formas de empezar:**
  - *Model-first* — escribes los archivos `.model` a mano (esta guía).
  - *Database-first* — ejecutas `molde pull` contra una base de datos
    existente y molde escribe los archivos `.model` por ti. Ideal para adoptar
    molde en un proyecto que ya tiene un esquema.

Un directorio `models/` típicamente se ve así:

```
models/
  database.model     # global settings (optional)
  Customer.model
  Order.model
```

---

## 2. Tu primer modelo

Crea `models/Customer.model`:

```
Customer
  fields:
    Id:    int pk
    Name:  string
    Email: string
```

Ese es un modelo válido: una tabla `Customer` con tres columnas e `Id` como
llave primaria. Llévalo a una base de datos (SQLite no necesita servidor):

```bash
molde migrate InitialCreate                         # create the migration
molde db create --connection "sqlite://app.db"      # create the database
molde apply     --connection "sqlite://app.db"      # apply it
```

La primera línea de todo archivo de entidad es el **header** — el nombre de la
entidad. Si el nombre físico de la tabla difiere del nombre de la entidad,
escríbelo después de dos puntos:

```
Customer: tbl_customers
  fields:
    ...
```

---

## 3. Campos y tipos

Las columnas van bajo `fields:`, una por línea, como `Nombre: tipo facets…`.

```
fields:
  Id:        int
  Name:      string
  Price:     decimal
  CreatedAt: datetimeoffset
  IsActive:  bool
```

molde usa **tipos lógicos** que cada motor mapea a su propio tipo nativo. Los
más comunes:

| Tipo lógico | SQL típico (PostgreSQL) |
|---|---|
| `int`, `long`, `short`, `byte` | integer, bigint, smallint |
| `bool` | boolean |
| `string` | text, o `varchar(n)` con `maxlen=` |
| `decimal` | numeric, o `numeric(p,s)` con `precision=` |
| `double`, `float` | double precision, real |
| `datetime`, `datetimeoffset` | timestamp, timestamptz |
| `date`, `time` | date, time |
| `guid` | uuid |
| `bytes` | bytea |
| `json` | jsonb |

(Consulta la [especificación](molde-language-spec.md#5-logical-types) para la
tabla completa.)

**¿Necesitas un tipo nativo exacto?** Escríbelo directamente — cualquier cosa
que molde no reconozca como tipo lógico se trata como el tipo literal de la
base de datos:

```
fields:
  Embedding: vector(1536)    # stored as exactly vector(1536)
  Search:    tsvector
```

O conserva un tipo lógico pero sobrescribe cómo se almacena con `dbtype=`:

```
fields:
  Payload: json dbtype=jsonb
```

---

## 4. Nulabilidad

Las columnas son **`NOT NULL` por defecto**. Agrega un `?` justo después del
tipo para hacer una columna nullable:

```
fields:
  Name:       string       # required
  MiddleName: string?      # optional (nullable)
```

No existe una palabra clave `required` — la ausencia de `?` ya significa
requerido.

---

## 5. Llaves primarias

**Columna única** — agrega el facet `pk`:

```
fields:
  Id: int pk
```

La restricción se nombra `pk_<table>` por convención. ¿Quieres un nombre
personalizado?

```
fields:
  Id: int pk=customers_pk
```

**Compuesta** (más de una columna) — usa el bloque `key:` en su lugar:

```
TenantItem
  fields:
    TenantId: int
    Id:       int
  key: [TenantId, Id]
```

Con un nombre personalizado:

```
  key: [TenantId, Id] name=tenant_items_pk
```

---

## 6. Identity / auto-incremento

Agrega `identity` para que la base de datos genere el valor (serial /
auto-incremento / IDENTITY, según el motor):

```
fields:
  Id: int pk identity
```

---

## 7. Facets de columna

Los facets son modificadores separados por espacios después del tipo. Los
facets booleanos son solo una palabra clave; el resto toma un `=valor`.

| Facet | Qué hace | Ejemplo |
|---|---|---|
| `pk` / `pk=<name>` | llave primaria (opcionalmente con nombre) | `Id: int pk` |
| `identity` | valor generado por la base de datos | `Id: int pk identity` |
| `unique` | índice unique de una sola columna | `Email: string unique` |
| `maxlen=<n>` | longitud máxima (→ `varchar(n)`) | `Name: string maxlen=200` |
| `precision=<p>[,<s>]` | precisión/escala numérica | `Total: decimal precision=18,2` |
| `default=<sql>` | valor por defecto (**SQL crudo**) | `IsActive: bool default=true` |
| `computed=<sql>` / `stored` | columna calculada (ver §8) | `Slug: string computed="lower(name)" stored` |
| `collation=<name>` | collation de la columna | `Code: string collation=C` |
| `comment="<text>"` | comentario de la columna | `Body: json comment="raw payload"` |
| `dbtype=<type>` | tipo de almacenamiento nativo exacto | `Payload: json dbtype=jsonb` |
| `clr=<type>` | sobrescribe el tipo .NET/CLR mapeado (avanzado; rara vez se necesita) | `Id: int clr=System.Int32` |

Algunas notas con ejemplos:

```
fields:
  Name:     string maxlen=200
  Total:    decimal precision=18,2
  IsActive: bool default=true
  Status:   string maxlen=20 default="'active'"   # SQL string literal → quote it
  CreatedAt: datetimeoffset default="now()"
  Notes:    string? comment="free-form notes"
```

`default=` es **SQL crudo**, así que un default de texto necesita comillas SQL
por dentro (`default="'active'"`), y puedes llamar funciones
(`default="now()"`). Encierra el valor completo entre comillas siempre que
contenga espacios.

---

## 8. Columnas calculadas

Una columna calculada (generada) deriva su valor de una expresión. Usa
`computed=<sql>`, y agrega `stored` si quieres que se materialice en disco (de
lo contrario es virtual donde el motor lo soporte):

```
fields:
  FirstName: string maxlen=100
  LastName:  string maxlen=100
  FullName:  string computed="FirstName || ' ' || LastName" stored
```

---

## 9. Restricciones unique

Para una **sola columna**, el facet `unique` es el atajo — crea un índice
unique de una sola columna llamado `ix_<table>_<col>`:

```
fields:
  Email: string? unique maxlen=320
```

Para una restricción unique **multi-columna**, declárala en el bloque
`indexes:` con `unique: true` (ver §11).

---

## 10. Relaciones (foreign keys)

Una foreign key vive en la tabla **dependiente** (la que contiene la
referencia), bajo `belongs-to:`. Cada entrada es `NavName: { … }`:

```
Order
  fields:
    Id:         int pk identity
    CustomerId: int
  belongs-to:
    Customer: {fk: CustomerId, references: Customer.Id, onDelete: cascade}
```

| Clave | Significado |
|---|---|
| etiqueta (`Customer`) | nombre de navegación — solo documental |
| `fk` | la(s) columna(s) local(es); una lista para composite: `[A, B]` |
| `references` | destino como `Table.Column` (o `schema.Table.[A, B]`) |
| `onDelete` | `no_action` \| `restrict` \| `cascade` \| `set_null` \| `set_default` |
| `name` | nombre de restricción opcional (por defecto `fk_<table>_<principal>`) |
| `index` | pon `false` para omitir el índice de respaldo automático |

**Las foreign keys se indexan automáticamente.** Cada `belongs-to` obtiene un
índice no unique en su(s) columna(s) — no lo escribes, y se mantiene oculto
del `.model` canónico. molde lo omite cuando esas columnas ya están cubiertas
(por ejemplo, cuando encabezan la llave primaria). Para excluirlo
explícitamente:

```
  belongs-to:
    Customer: {fk: CustomerId, references: Customer.Id, index: false}
```

**Foreign key compuesta:**

```
  belongs-to:
    Tenant: {fk: [TenantId, OrgId], references: Org.[TenantId, Id], onDelete: restrict}
```

Solo se modela el lado dependiente — no hay un lado "has-many"/inverso que
declarar. La relación queda completamente descrita por la FK.

---

## 11. Índices

Tres tipos de índices, tres formas de obtenerlos:

1. **Índices de foreign key** — automáticos (§10). No los escribes.
2. **Unique de una sola columna** — el facet `unique` (§9).
3. **Todo lo demás** — declarado en el bloque `indexes:`: índices de
   rendimiento, compuestos, unique multi-columna, parciales/filtrados,
   métodos especiales, e índices de expresión/full-text.

Cada entrada es `- <name>: { … }`, donde la etiqueta es el nombre del índice
(minúscula por convención) y `on:` es la lista ordenada de columnas:

```
indexes:
  - ix_order_status_created: {on: [Status, CreatedAt]}            # composite
  - ix_order_number:         {on: [Number], unique: true}        # multi-col unique
  - ix_order_active:         {on: [Status], filter: "Deleted = false"}  # partial
  - ix_docs_fts:             {on: [], expression: "to_tsvector('english', Body)", method: gin}
  - ix_emb:                  {on: [Embedding], method: hnsw, operators: [vector_cosine_ops]}
```

| Clave | Significado |
|---|---|
| etiqueta | nombre del índice |
| `on` | lista ordenada de columnas |
| `unique` | `true` para un índice unique |
| `filter` | predicado para un índice parcial/filtrado |
| `method` | método de índice: `gin`, `gist`, `hnsw`, `ivfflat`, … |
| `operators` | clase de operador por columna (por ejemplo, `vector_cosine_ops`) |
| `expression` | expresión funcional / full-text (tiene precedencia sobre `on`) |

---

## 12. Datos seed

Las filas de referencia/lookup que siempre deben existir van bajo `seed:`.
Cada fila es un objeto inline de `Columna: valor`; usa `~` para null:

```
seed:
  - {Id: 1, Name: "ACME",   Email: "acme@example.com"}
  - {Id: 2, Name: "Globex", Email: ~}
```

molde convierte las filas seed en `INSERT`/`UPDATE`/`DELETE` a medida que los
datos cambian entre migraciones — un upsert indexado por la **llave
primaria**: agregar una fila → `INSERT`, cambiar un valor en una llave
existente → `UPDATE`, quitar una fila → `DELETE`. Así que toda fila seed debe
incluir su llave primaria. Las filas seed describen **datos**, por lo que se
ignoran intencionalmente en la verificación de drift del esquema
(`molde verify`).

> **Los seeds viven con su tabla — no hay archivos de seed separados.** Coloca
> el bloque `seed:` en el propio archivo `.model` de la entidad. Cada archivo
> `.model` es una tabla, así que un segundo archivo que repita el mismo header
> de entidad solo para contener seeds definiría una tabla *duplicada*, no
> agregaría datos a la existente. (Los archivos de seed separados/por entorno
> y el seeding desde CSV/JSON están en el roadmap, aún no soportados.)

### Seeding cuando la base de datos genera la llave (`seed-key`)

Por defecto los seeds hacen match por la **llave primaria**, así que cada fila
debe incluirla. Eso es un problema cuando la PK la genera la base de datos —
por ejemplo, un `guid` con default `gen_random_uuid()` — porque no tienes el
valor para escribirlo.

Declara una **llave natural** con `seed-key:` y molde hace match de las filas
por esa columna en su lugar, dejándote omitir por completo la PK generada:

```
Tenant
  fields:
    Id:   guid pk default="gen_random_uuid()"
    Code: string maxlen=40 unique
    Name: string maxlen=100
  seed-key: [Code]            # match seed rows by Code, not by Id
  seed:
    - {Code: "ACME", Name: "ACME Inc"}   # no Id — the database generates it
    - {Code: "GLX",  Name: "Globex"}
```

Ahora los `INSERT` omiten `Id` (la base de datos lo completa), y a través de
las migraciones molde hace upsert por `Code`: agregar una fila → `INSERT` solo
de esa; cambiar un `Name` para un `Code` existente → `UPDATE`; quitar una fila
→ `DELETE`. Las columnas de `seed-key` deben ser **unique** (aquí `Code` tiene
`unique`) para que el match sea inequívoco.

> Cuándo usar cuál: fija el `Id` a mano (un [UUIDv5](https://datatracker.ietf.org/doc/html/rfc4122)
> estable, idealmente determinístico) si quieres la *misma* llave en todos los
> entornos — la elección clásica para datos de referencia. Usa `seed-key`
> cuando te parece bien que cada entorno genere su propia llave y prefieras
> hacer match por un valor de negocio.

> **`pull` no trae de vuelta los seeds ni `seed-key`.** La introspección solo
> lee la *estructura* de la base de datos — nunca lee filas de datos, y
> `seed-key` no es un objeto de base de datos — así que un modelo producido
> por `molde pull` no tiene bloques `seed:`/`seed-key:` (igual que los atajos
> `owns`/`enum`/`subtypes` del §15). Tus archivos `.model` escritos a mano
> siguen siendo la fuente de verdad para los datos seed.

---

## 13. Triggers

Los triggers de base de datos adjuntos a una tabla van bajo `triggers:`. El
bloque `sql:` es la fuente de verdad — molde lo almacena tal cual y lo
recrea:

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

| Clave | Significado |
|---|---|
| etiqueta | nombre del trigger |
| `timing` | `before` \| `after` \| `instead_of` |
| `events` | cualquiera de `insert`, `update`, `delete`, `truncate` |
| `function` | la función que llama (informativo) |
| `sql` | el DDL completo de `CREATE TRIGGER` (tal cual) |

La función que llama el trigger normalmente se define en `database.model`
(§14).

---

## 14. El archivo global `database.model`

La configuración compartida por toda la base de datos va en
`models/database.model`. Cada sección es opcional:

```
schema: public                     # default schema for all tables
product-version: 16.0              # informational
extensions: [pg_trgm, unaccent, vector]
functions:
  - normalize_body: |
      CREATE OR REPLACE FUNCTION public.normalize_body() RETURNS trigger
      LANGUAGE plpgsql AS $$ BEGIN NEW.body := lower(NEW.body); RETURN NEW; END $$
raw:
  - |
      CREATE FULLTEXT CATALOG ft AS DEFAULT
```

| Sección | Significado |
|---|---|
| `schema` | esquema por defecto para las tablas que no definen el suyo |
| `product-version` | etiqueta de versión informativa |
| `extensions` | extensiones de base de datos a garantizar que existan (por ejemplo, `vector` en PostgreSQL) |
| `functions` | funciones reutilizables (cada una un nombre + DDL tal cual) |
| `raw` | escape hatch: DDL tal cual que molde aplica sin modificar |

Una sola tabla puede sobrescribir el esquema por defecto con su propia línea
`schema:`.

---

## 15. Atajos de autoría

Estos son **azúcar sintáctica**: convenientes de escribir, pero se expanden a
columnas planas al parsearse. El scaffolding (`molde pull`) siempre emite la
forma expandida — así que no esperes que `owns`/`enum`/`subtypes` vuelvan a
salir de una base de datos introspeccionada.

### Owned types

Agrupa unas cuantas columnas relacionadas bajo un prefijo:

```
fields:
  Contact: owns ContactInfo {Phone: string?, City: string? maxlen=80}
```

Se expande a las columnas planas `Contact_Phone` y `Contact_City`.

### Enums

Un conjunto restringido de valores, almacenado como string o int:

```
fields:
  Status: enum[Pending, Shipped, Delivered] as=string maxlen=20
```

`as=string` lo almacena como texto, `as=int` como número. Los valores listados
son documentación — molde no agrega una restricción `CHECK` por defecto.

### Herencia de tabla única (TPH)

Una tabla para un tipo base más sus subtipos, distinguidos por una columna
discriminadora. Las columnas del subtipo se vuelven nullable:

```
Payment
  discriminator: Discriminator
  fields:
    Id:     int pk identity
    Amount: decimal precision=18,2
  subtypes:
    CardPayment: {CardNumber: string maxlen=20}
    CashPayment: {Note: string?}
```

Se expande a una única tabla `Payment` con `Id`, `Amount`, `CardNumber?`,
`Note?`, y una columna `Discriminator`.

---

## 16. Tipos específicos de motor y escapes

molde es agnóstico de motor, pero puedes bajar a una base de datos específica
cuando lo necesites:

- **Tipo nativo exacto** — escríbelo como el tipo, por ejemplo `vector(1536)`,
  `tsvector`, `citext`, `jsonb`, `int[]`.
- **`dbtype=`** — conserva un tipo lógico pero fija el tipo de almacenamiento:
  `json dbtype=jsonb`.
- **`raw:`** en `database.model` — DDL tal cual para cualquier cosa que el
  modelo no pueda expresar.

Ejemplo de búsqueda vectorial en PostgreSQL, combinando varias
características:

`database.model`
```
schema: public
extensions: [vector]
```

`Document.model`
```
Document
  fields:
    Id:        long pk identity
    Body:      string
    Embedding: vector(1536)
  indexes:
    - ix_document_embedding: {on: [Embedding], method: hnsw, operators: [vector_cosine_ops]}
```

---

## 17. El flujo de trabajo del día a día

Una vez que tus modelos están escritos:

```bash
molde fmt                                  # canonicalize formatting (optional)
molde migrate AddDocuments                 # diff models → a new migration
molde lint                                 # check the migration for risky changes
molde apply --connection "$DATABASE_URL"   # apply pending migrations
molde verify --connection "$DATABASE_URL"  # confirm the DB matches the models
```

¿Editando modelos más tarde? Cambia los archivos `.model`, corre `molde
migrate <Name>` de nuevo, y molde escribe una migración con solo la
diferencia. Para adoptar molde en una base de datos existente en su lugar,
empieza con `molde pull`. Consulta la [referencia de CLI](cli.md) para cada
comando y flag.

---

## 18. Un ejemplo completo resuelto

Una tienda pequeña, distribuida en los archivos convencionales.

`models/database.model`
```
schema: public
```

`models/Customer.model`
```
Customer
  fields:
    Id:    int pk identity
    Name:  string maxlen=200
    Email: string? unique maxlen=320
  seed:
    - {Id: 1, Name: "ACME",   Email: "acme@example.com"}
    - {Id: 2, Name: "Globex", Email: ~}
```

`models/Product.model`
```
Product
  fields:
    Id:       int pk identity
    Sku:      string maxlen=40 unique
    Name:     string maxlen=200
    Price:    decimal precision=18,2
    IsActive: bool default=true
```

`models/Order.model`
```
Order
  fields:
    Id:         int pk identity
    CustomerId: int
    Status:     enum[Pending, Paid, Shipped, Cancelled] as=string maxlen=20 default="'Pending'"
    Total:      decimal precision=18,2
    CreatedAt:  datetimeoffset default="now()"
  belongs-to:
    Customer: {fk: CustomerId, references: Customer.Id, onDelete: cascade}
  indexes:
    - ix_order_status_created: {on: [Status, CreatedAt]}
```

`models/OrderLine.model`
```
OrderLine
  fields:
    Id:        int pk identity
    OrderId:   int
    ProductId: int
    Quantity:  int default=1
    UnitPrice: decimal precision=18,2
  belongs-to:
    Order:   {fk: OrderId,   references: Order.Id,   onDelete: cascade}
    Product: {fk: ProductId, references: Product.Id, onDelete: restrict}
```

Dale vida en SQLite:

```bash
molde migrate InitialCreate
molde db create --connection "sqlite://store.db"
molde apply     --connection "sqlite://store.db"
molde status
```

Apunta `--connection` a PostgreSQL, MySQL o SQL Server para correr exactamente
los mismos modelos contra otro motor.

---

**Siguiente:** la [especificación del lenguaje](molde-language-spec.md) para
la gramática precisa y el mapeo a IR, y la [referencia de CLI](cli.md) para el
conjunto completo de comandos.
```
