-- Volcado normalizado del esquema de Postgres para comparar paridad.
-- Ordenado de forma determinista y excluyendo la tabla de historial de
-- migraciones (cada herramienta la gestiona a su manera).
\pset tuples_only on
\pset format unaligned
\pset fieldsep '|'

SELECT 'TABLE', table_name
FROM information_schema.tables
WHERE table_schema = 'public' AND table_type = 'BASE TABLE'
  AND table_name <> '__EFMigrationsHistory'
ORDER BY table_name;

SELECT 'COLUMN', table_name, column_name, data_type, is_nullable,
       coalesce(character_maximum_length::text, '-'),
       is_identity
FROM information_schema.columns
WHERE table_schema = 'public' AND table_name <> '__EFMigrationsHistory'
ORDER BY table_name, column_name;

SELECT 'PK', tc.table_name, kcu.column_name
FROM information_schema.table_constraints tc
JOIN information_schema.key_column_usage kcu
  ON tc.constraint_name = kcu.constraint_name AND tc.table_schema = kcu.table_schema
WHERE tc.table_schema = 'public' AND tc.constraint_type = 'PRIMARY KEY'
  AND tc.table_name <> '__EFMigrationsHistory'
ORDER BY tc.table_name, kcu.column_name;

SELECT 'FK', tc.table_name, ccu.table_name AS ref_table, rc.delete_rule
FROM information_schema.table_constraints tc
JOIN information_schema.referential_constraints rc
  ON tc.constraint_name = rc.constraint_name AND tc.constraint_schema = rc.constraint_schema
JOIN information_schema.constraint_column_usage ccu
  ON ccu.constraint_name = tc.constraint_name AND ccu.constraint_schema = tc.constraint_schema
WHERE tc.table_schema = 'public' AND tc.constraint_type = 'FOREIGN KEY'
ORDER BY tc.table_name, ref_table;

SELECT 'INDEX', tablename, indexname
FROM pg_indexes
WHERE schemaname = 'public' AND tablename <> '__EFMigrationsHistory'
ORDER BY tablename, indexname;
