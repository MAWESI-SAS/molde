namespace SampleModel;

public class Customer
{
    public int Id { get; set; }
    public string Name { get; set; } = null!;
    public string? Email { get; set; }

    public ICollection<Order> Orders { get; set; } = new List<Order>();
}

public class Order
{
    public int Id { get; set; }
    public int CustomerId { get; set; }
    public decimal Total { get; set; }

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
