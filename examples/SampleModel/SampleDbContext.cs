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
        });

        modelBuilder.Entity<Order>(e =>
        {
            e.ToTable("Order");
            e.HasKey(x => x.Id).HasName("PK_Order");
            e.Property(x => x.Total).HasColumnType("TEXT");
            e.HasOne(x => x.Customer)
                .WithMany(c => c.Orders)
                .HasForeignKey(x => x.CustomerId)
                .HasConstraintName("FK_Order_Customer")
                .OnDelete(DeleteBehavior.Cascade);
        });
    }
}
