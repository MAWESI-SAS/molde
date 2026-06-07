namespace SampleModel;

public class Customer
{
    public int Id { get; set; }
    public string Name { get; set; } = null!;
    public string? Email { get; set; }

    // Tipo owned (OwnsOne): se embebe como columnas en la tabla Customer.
    public ContactInfo? Contact { get; set; }

    public ICollection<Order> Orders { get; set; } = new List<Order>();
}

// Tipo owned (sin tabla propia): sus propiedades viven en la tabla del owner.
public class ContactInfo
{
    public string? Phone { get; set; }
}

public enum OrderStatus
{
    Pending,
    Shipped,
}

public class Order
{
    public int Id { get; set; }
    public int CustomerId { get; set; }
    public decimal Total { get; set; }

    // Value converter: enum almacenado como string (afecta el store_type).
    public OrderStatus Status { get; set; }

    public Customer Customer { get; set; } = null!;
}

// Jerarquía TPH (table-per-hierarchy): base + derivados comparten una tabla con
// una columna discriminadora. El sidecar debe fundirlos en UNA sola tabla.
public abstract class Payment
{
    public int Id { get; set; }
    public decimal Amount { get; set; }
}

public class CardPayment : Payment
{
    public string CardNumber { get; set; } = null!;
}

public class CashPayment : Payment
{
    public string? Note { get; set; }
}
