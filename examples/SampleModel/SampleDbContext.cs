using Microsoft.EntityFrameworkCore;

namespace SampleModel;

public class SampleDbContext : DbContext
{
    public SampleDbContext(DbContextOptions<SampleDbContext> options)
        : base(options)
    {
    }

    public DbSet<Customer> Customers => Set<Customer>();
    public DbSet<Order> Orders => Set<Order>();
    public DbSet<Payment> Payments => Set<Payment>();

    // Configuración SOLO por Fluent API (sin data annotations): es justo lo que
    // un analizador estático de C# no podría resolver y que EF sí evalúa.
    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<Customer>(e =>
        {
            e.ToTable("Customer");
            e.HasKey(x => x.Id).HasName("PK_Customer");
            e.Property(x => x.Name).HasMaxLength(200).IsRequired();
            e.Property(x => x.Email).HasMaxLength(320);
            e.HasIndex(x => x.Email, "IX_Customer_Email").IsUnique();
            // Datos sembrados (HasData): el sidecar los extrae a seed_data.
            e.HasData(
                new Customer { Id = 1, Name = "ACME", Email = "acme@example.com" },
                new Customer { Id = 2, Name = "Globex", Email = null });
            // Tipo owned embebido: columnas Contact_Phone en la tabla Customer.
            e.OwnsOne(x => x.Contact);
        });

        // TPH: una tabla "Payment" con discriminador para base + derivados.
        modelBuilder.Entity<Payment>(e =>
        {
            e.ToTable("Payment");
            e.HasKey(x => x.Id).HasName("PK_Payment");
        });
        // Registrar los tipos derivados para que EF construya la jerarquía TPH.
        modelBuilder.Entity<CardPayment>();
        modelBuilder.Entity<CashPayment>();

        modelBuilder.Entity<Order>(e =>
        {
            e.ToTable("Order");
            e.HasKey(x => x.Id).HasName("PK_Order");
            e.Property(x => x.Total).HasColumnType("TEXT");
            // Value converter: el enum se guarda como string.
            e.Property(x => x.Status).HasConversion<string>().HasMaxLength(20);
            e.HasOne(x => x.Customer)
                .WithMany(c => c.Orders)
                .HasForeignKey(x => x.CustomerId)
                .HasConstraintName("FK_Order_Customer")
                .OnDelete(DeleteBehavior.Cascade);
        });
    }
}
