#!/usr/bin/env bash
# Suite de paridad: aplica el MISMO modelo (examples/SampleModel) con `dotnet ef`
# y con `efrust` a dos bases Postgres distintas y compara los esquemas resultantes.
#
# Requisitos (los provee el contenedor de CI/devcontainer):
#   - dotnet SDK 9 + dotnet-ef en el PATH
#   - binario efrust ya compilado (target/debug/efrust)
#   - Postgres accesible (variable PGHOST)
#
# Variables: PGHOST (host de Postgres), PGUSER/PGPASS (credenciales).
set -euo pipefail

PGHOST=${PGHOST:-efrust-pg}
PGUSER=${PGUSER:-efrust}
PGPASS=${PGPASS:-efrust}
EF_DB=ef_parity
RUST_DB=efrust_parity

SAMPLE=examples/SampleModel
SIDE_DLL=$(ls sidecar/EfRust.Sidecar/bin/Release/net9.0/efrust-sidecar.dll)
SAMPLE_DLL=$(ls "$SAMPLE"/bin/Release/net9.0/SampleModel.dll)
BIN=./target/debug/efrust

CONN_EF="Host=$PGHOST;Database=$EF_DB;Username=$PGUSER;Password=$PGPASS"
URL_RUST="postgres://$PGUSER:$PGPASS@$PGHOST:5432/$RUST_DB"
export PGPASSWORD="$PGPASS"

echo "==> Preparando bases de datos limpias"
psql -h "$PGHOST" -U "$PGUSER" -d postgres -c "DROP DATABASE IF EXISTS $EF_DB;" -c "CREATE DATABASE $EF_DB;" >/dev/null
psql -h "$PGHOST" -U "$PGUSER" -d postgres -c "DROP DATABASE IF EXISTS $RUST_DB;" -c "CREATE DATABASE $RUST_DB;" >/dev/null

echo "==> [EF] dotnet ef migrations add + database update"
rm -rf "$SAMPLE/Migrations"
dotnet ef migrations add Parity --project "$SAMPLE" >/dev/null
dotnet ef database update --project "$SAMPLE" --connection "$CONN_EF" >/dev/null

echo "==> [efrust] migrations add (sidecar) + database update"
rm -rf /tmp/parity-mig
EFRUST_SIDECAR="$SIDE_DLL" $BIN migrations add Parity \
    --assembly "$SAMPLE_DLL" --output-dir /tmp/parity-mig >/dev/null
$BIN database update --connection "$URL_RUST" --migrations-dir /tmp/parity-mig >/dev/null

echo "==> Volcando y comparando esquemas"
psql -h "$PGHOST" -U "$PGUSER" -d "$EF_DB" -f scripts/schema-dump.sql > /tmp/ef-schema.txt
psql -h "$PGHOST" -U "$PGUSER" -d "$RUST_DB" -f scripts/schema-dump.sql > /tmp/rust-schema.txt

echo "--- esquema (idéntico en ambas si hay paridad) ---"
cat /tmp/ef-schema.txt

# Limpieza de los archivos de migración que EF escribe en el proyecto.
rm -rf "$SAMPLE/Migrations"

if diff -u /tmp/ef-schema.txt /tmp/rust-schema.txt; then
    echo "✅ PARIDAD: los esquemas de dotnet ef y efrust son idénticos."
else
    echo "❌ DIFERENCIAS encontradas (ver diff arriba)."
    exit 1
fi
