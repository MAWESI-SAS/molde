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
