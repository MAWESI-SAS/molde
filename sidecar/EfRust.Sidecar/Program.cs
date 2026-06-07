// efrust-sidecar
//
// Único componente NO-Rust del sistema. Carga el assembly compilado del usuario,
// instancia su DbContext (vía IDesignTimeDbContextFactory<> o constructor sin
// parámetros), deja que EF Core resuelva el modelo (Fluent API + data annotations
// + convenciones) y emite el modelo RELACIONAL como JSON con EXACTAMENTE la forma
// del Model IR de `efrust-core` (ver crates/efrust-core/src/model.rs).
//
// Uso:
//   efrust-sidecar --assembly <ruta.dll> [--context <NombreDbContext>]
// Salida: JSON del modelo por stdout (los diagnósticos van a stderr).

using System.Reflection;
using System.Runtime.Loader;
using System.Text.Json;
using System.Text.Json.Serialization;
using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Design;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Metadata;

namespace EfRust.Sidecar;

// ---------------------------------------------------------------------------
// DTOs — espejo EXACTO del Model IR de Rust. snake_case para casar con serde.
// ---------------------------------------------------------------------------

internal sealed class DatabaseModelDto
{
    [JsonPropertyName("format_version")] public int FormatVersion { get; set; } = 1;
    [JsonPropertyName("product_version")] public string? ProductVersion { get; set; }
    [JsonPropertyName("default_schema")] public string? DefaultSchema { get; set; }
    [JsonPropertyName("tables")] public List<TableDto> Tables { get; set; } = new();
}

internal sealed class TableDto
{
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("schema")] public string? Schema { get; set; }
    [JsonPropertyName("clr_type")] public string? ClrType { get; set; }
    [JsonPropertyName("comment")] public string? Comment { get; set; }
    [JsonPropertyName("columns")] public List<ColumnDto> Columns { get; set; } = new();
    [JsonPropertyName("primary_key")] public PrimaryKeyDto? PrimaryKey { get; set; }
    [JsonPropertyName("foreign_keys")] public List<ForeignKeyDto> ForeignKeys { get; set; } = new();
    [JsonPropertyName("indexes")] public List<IndexDto> Indexes { get; set; } = new();
    [JsonPropertyName("seed_data")] public List<Dictionary<string, object?>> SeedData { get; set; } = new();
}

internal sealed class ColumnDto
{
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("store_type")] public string? StoreType { get; set; }
    [JsonPropertyName("clr_type")] public string? ClrType { get; set; }
    [JsonPropertyName("is_nullable")] public bool IsNullable { get; set; }
    [JsonPropertyName("is_identity")] public bool IsIdentity { get; set; }
    [JsonPropertyName("max_length")] public int? MaxLength { get; set; }
    [JsonPropertyName("precision")] public int? Precision { get; set; }
    [JsonPropertyName("scale")] public int? Scale { get; set; }
    [JsonPropertyName("default_value_sql")] public string? DefaultValueSql { get; set; }
    [JsonPropertyName("computed_sql")] public string? ComputedSql { get; set; }
    [JsonPropertyName("computed_stored")] public bool ComputedStored { get; set; }
    [JsonPropertyName("collation")] public string? Collation { get; set; }
    [JsonPropertyName("comment")] public string? Comment { get; set; }
}

internal sealed class PrimaryKeyDto
{
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("columns")] public List<string> Columns { get; set; } = new();
}

internal sealed class ForeignKeyDto
{
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("columns")] public List<string> Columns { get; set; } = new();
    [JsonPropertyName("principal_table")] public string PrincipalTable { get; set; } = "";
    [JsonPropertyName("principal_schema")] public string? PrincipalSchema { get; set; }
    [JsonPropertyName("principal_columns")] public List<string> PrincipalColumns { get; set; } = new();
    // Debe coincidir con el enum serde: no_action|restrict|cascade|set_null|set_default
    [JsonPropertyName("on_delete")] public string OnDelete { get; set; } = "no_action";
}

internal sealed class IndexDto
{
    [JsonPropertyName("name")] public string Name { get; set; } = "";
    [JsonPropertyName("columns")] public List<string> Columns { get; set; } = new();
    [JsonPropertyName("is_unique")] public bool IsUnique { get; set; }
    [JsonPropertyName("filter")] public string? Filter { get; set; }
}

// ---------------------------------------------------------------------------
// Entry point
// ---------------------------------------------------------------------------

internal static class Program
{
    private static readonly JsonSerializerOptions JsonOpts = new()
    {
        WriteIndented = true,
        DefaultIgnoreCondition = JsonIgnoreCondition.Never,
    };

    private static int Main(string[] args)
    {
        try
        {
            var (assemblyPath, contextName) = ParseArgs(args);
            if (assemblyPath is null)
            {
                Console.Error.WriteLine(
                    "uso: efrust-sidecar --assembly <ruta.dll> [--context <NombreDbContext>]");
                return 2;
            }

            var assembly = LoadAssembly(assemblyPath);
            using var context = ContextActivator.Create(assembly, contextName);
            // El modelo design-time incluye todos los metadatos (comentarios, etc.),
            // a diferencia del modelo read-optimized de runtime (context.Model).
            var designTimeModel = context.GetService<IDesignTimeModel>().Model;
            var model = ModelMapper.Map(designTimeModel);

            Console.WriteLine(JsonSerializer.Serialize(model, JsonOpts));
            return 0;
        }
        catch (Exception ex)
        {
            // Desenvolver TargetInvocationException (reflexión) para ver la causa real.
            var root = ex;
            while (root.InnerException is not null)
            {
                root = root.InnerException;
            }
            Console.Error.WriteLine($"efrust-sidecar error: {root.GetType().Name}: {root.Message}");
            Console.Error.WriteLine(root.StackTrace);
            return 1;
        }
    }

    private static (string? assembly, string? context) ParseArgs(string[] args)
    {
        string? assembly = null, context = null;
        for (var i = 0; i < args.Length; i++)
        {
            switch (args[i])
            {
                case "--assembly" when i + 1 < args.Length:
                    assembly = args[++i];
                    break;
                case "--context" when i + 1 < args.Length:
                    context = args[++i];
                    break;
            }
        }
        return (assembly, context);
    }

    /// Carga el assembly objetivo resolviendo sus dependencias (incluido el
    /// provider EF, p. ej. Sqlite/Npgsql) desde su carpeta de salida.
    private static Assembly LoadAssembly(string path)
    {
        var fullPath = Path.GetFullPath(path);
        var dir = Path.GetDirectoryName(fullPath)!;
        var resolver = new AssemblyDependencyResolver(fullPath);
        AssemblyLoadContext.Default.Resolving += (ctx, name) =>
        {
            // 1. Vía deps.json (AssemblyDependencyResolver).
            var dep = resolver.ResolveAssemblyToPath(name);
            // 2. Fallback: DLL junto al assembly objetivo.
            if (dep is null && name.Name is not null)
            {
                var candidate = Path.Combine(dir, name.Name + ".dll");
                if (File.Exists(candidate))
                {
                    dep = candidate;
                }
            }
            return dep is null ? null : ctx.LoadFromAssemblyPath(dep);
        };
        return AssemblyLoadContext.Default.LoadFromAssemblyPath(fullPath);
    }
}

// ---------------------------------------------------------------------------
// Instanciación del DbContext (contrato design-time de EF)
// ---------------------------------------------------------------------------

internal static class ContextActivator
{
    public static DbContext Create(Assembly assembly, string? contextName)
    {
        var contextTypes = assembly
            .GetTypes()
            .Where(t => typeof(DbContext).IsAssignableFrom(t) && !t.IsAbstract)
            .ToList();

        if (contextName is not null)
        {
            contextTypes = contextTypes
                .Where(t => t.Name == contextName || t.FullName == contextName)
                .ToList();
        }

        if (contextTypes.Count == 0)
        {
            throw new InvalidOperationException(
                "no se encontró ningún DbContext en el assembly indicado.");
        }
        if (contextTypes.Count > 1)
        {
            throw new InvalidOperationException(
                $"se encontraron varios DbContext ({string.Join(", ", contextTypes.Select(t => t.Name))}); "
                + "especifica cuál con --context.");
        }

        var contextType = contextTypes[0];

        // 1. IDesignTimeDbContextFactory<TContext> (vía recomendada por EF).
        var factoryType = assembly.GetTypes().FirstOrDefault(t =>
            !t.IsAbstract
            && t.GetInterfaces().Any(i =>
                i.IsGenericType
                && i.GetGenericTypeDefinition() == typeof(IDesignTimeDbContextFactory<>)
                && i.GetGenericArguments()[0] == contextType));

        if (factoryType is not null)
        {
            var factory = Activator.CreateInstance(factoryType)!;
            var create = factoryType.GetMethod("CreateDbContext", new[] { typeof(string[]) })!;
            return (DbContext)create.Invoke(factory, new object[] { Array.Empty<string>() })!;
        }

        // 2. Constructor sin parámetros (depende de OnConfiguring).
        var ctor = contextType.GetConstructor(Type.EmptyTypes);
        if (ctor is not null)
        {
            return (DbContext)ctor.Invoke(null);
        }

        throw new InvalidOperationException(
            $"no se pudo instanciar '{contextType.Name}': añade un "
            + "IDesignTimeDbContextFactory<> o un constructor sin parámetros con OnConfiguring.");
    }
}

// ---------------------------------------------------------------------------
// Mapeo IModel (EF) -> DTOs (Model IR)
// ---------------------------------------------------------------------------

internal static class ModelMapper
{
    public static DatabaseModelDto Map(IModel model)
    {
        var dto = new DatabaseModelDto
        {
            ProductVersion = typeof(DbContext).Assembly.GetName().Version?.ToString(),
            DefaultSchema = model.GetDefaultSchema(),
        };

        // Agrupar tipos de entidad por tabla física. En herencia TPH varios tipos
        // CLR (base + derivados) comparten la MISMA tabla; deben fundirse en un
        // único TableDto (columnas/FKs/índices unidos, columna discriminadora
        // incluida como una columna normal) en vez de emitir tablas duplicadas.
        var groups = new Dictionary<string, List<IEntityType>>();
        foreach (var entity in model.GetEntityTypes())
        {
            var tableName = entity.GetTableName();
            if (tableName is null)
            {
                continue; // keyless / sin tabla (owned sin tabla propia, vistas, etc.)
            }
            var key = $"{entity.GetSchema()}.{tableName}";
            if (!groups.TryGetValue(key, out var list))
            {
                list = new List<IEntityType>();
                groups[key] = list;
            }
            list.Add(entity);
        }

        foreach (var group in groups.Values)
        {
            // La raíz (BaseType == null) define nombre/PK/clr_type; si no hay raíz
            // explícita en el grupo, se usa el primero.
            var root = group.FirstOrDefault(e => e.BaseType is null) ?? group[0];
            var tableName = root.GetTableName()!;
            var schema = root.GetSchema();
            var store = StoreObjectIdentifier.Table(tableName, schema);

            var table = new TableDto
            {
                Name = tableName,
                Schema = schema,
                ClrType = root.ClrType.FullName,
                Comment = root.GetComment(),
            };

            // Columnas: unión sobre todos los tipos del grupo (dedup por nombre de
            // columna). Las propiedades de tipos derivados son columnas nullable.
            var seenCols = new HashSet<string>();
            foreach (var entity in group)
            {
                foreach (var p in entity.GetProperties())
                {
                    var colName = p.GetColumnName(store) ?? p.Name;
                    if (!seenCols.Add(colName))
                    {
                        continue;
                    }
                    var clr = Nullable.GetUnderlyingType(p.ClrType) ?? p.ClrType;
                    table.Columns.Add(new ColumnDto
                    {
                        Name = colName,
                        StoreType = p.GetColumnType(store),
                        ClrType = clr.FullName,
                        IsNullable = p.IsColumnNullable(store),
                        IsIdentity = p.ValueGenerated == ValueGenerated.OnAdd && IsInteger(clr),
                        MaxLength = p.GetMaxLength(),
                        Precision = p.GetPrecision(),
                        Scale = p.GetScale(),
                        DefaultValueSql = p.GetDefaultValueSql(store),
                        ComputedSql = p.GetComputedColumnSql(store),
                        ComputedStored = p.GetIsStored(store) ?? false,
                        Collation = p.GetCollation(),
                        Comment = p.GetComment(store),
                    });
                }
            }

            var pk = root.FindPrimaryKey();
            if (pk is not null)
            {
                table.PrimaryKey = new PrimaryKeyDto
                {
                    Name = pk.GetName(store) ?? $"PK_{tableName}",
                    Columns = pk.Properties.Select(pp => pp.GetColumnName(store) ?? pp.Name).ToList(),
                };
            }

            var seenFks = new HashSet<string>();
            foreach (var entity in group)
            {
                foreach (var fk in entity.GetForeignKeys())
                {
                    var principal = fk.PrincipalEntityType;
                    var principalTable = principal.GetTableName();
                    if (principalTable is null)
                    {
                        continue;
                    }
                    var principalStore = StoreObjectIdentifier.Table(principalTable, principal.GetSchema());
                    var name = fk.GetConstraintName(store, principalStore)
                        ?? $"FK_{tableName}_{principalTable}";
                    if (!seenFks.Add(name))
                    {
                        continue;
                    }
                    table.ForeignKeys.Add(new ForeignKeyDto
                    {
                        Name = name,
                        Columns = fk.Properties.Select(pp => pp.GetColumnName(store) ?? pp.Name).ToList(),
                        PrincipalTable = principalTable,
                        PrincipalSchema = principal.GetSchema(),
                        PrincipalColumns = fk.PrincipalKey.Properties
                            .Select(pp => pp.GetColumnName(principalStore) ?? pp.Name).ToList(),
                        OnDelete = MapDeleteBehavior(fk.DeleteBehavior),
                    });
                }
            }

            var seenIx = new HashSet<string>();
            foreach (var entity in group)
            {
                foreach (var ix in entity.GetIndexes())
                {
                    var name = ix.GetDatabaseName(store) ?? "";
                    if (!seenIx.Add(name))
                    {
                        continue;
                    }
                    table.Indexes.Add(new IndexDto
                    {
                        Name = name,
                        Columns = ix.Properties.Select(pp => pp.GetColumnName(store) ?? pp.Name).ToList(),
                        IsUnique = ix.IsUnique,
                        Filter = ix.GetFilter(store),
                    });
                }
            }

            // Datos sembrados (HasData) de todos los tipos del grupo.
            foreach (var entity in group)
            {
                foreach (var seed in entity.GetSeedData())
                {
                    var row = new Dictionary<string, object?>();
                    foreach (var p in entity.GetProperties())
                    {
                        if (seed.TryGetValue(p.Name, out var val))
                        {
                            row[p.GetColumnName(store) ?? p.Name] = val;
                        }
                    }
                    if (row.Count > 0)
                    {
                        table.SeedData.Add(row);
                    }
                }
            }

            dto.Tables.Add(table);
        }

        dto.Tables.Sort((a, b) =>
            string.CompareOrdinal($"{a.Schema}.{a.Name}", $"{b.Schema}.{b.Name}"));
        return dto;
    }

    private static bool IsInteger(Type t) =>
        t == typeof(int) || t == typeof(long) || t == typeof(short) || t == typeof(byte);

    private static string MapDeleteBehavior(DeleteBehavior behavior) => behavior switch
    {
        DeleteBehavior.Cascade => "cascade",
        DeleteBehavior.SetNull => "set_null",
        DeleteBehavior.Restrict => "restrict",
        _ => "no_action",
    };
}
