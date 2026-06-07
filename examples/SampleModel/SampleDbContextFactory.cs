using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Design;

namespace SampleModel;

// El sidecar usa esta factory (contrato design-time de EF) para instanciar el
// DbContext con su provider real. Construir el modelo NO abre conexión, así que
// la cadena es nominal.
public class SampleDbContextFactory : IDesignTimeDbContextFactory<SampleDbContext>
{
    public SampleDbContext CreateDbContext(string[] args)
    {
        var options = new DbContextOptionsBuilder<SampleDbContext>()
            .UseNpgsql("Host=localhost;Database=sample")
            .Options;
        return new SampleDbContext(options);
    }
}
