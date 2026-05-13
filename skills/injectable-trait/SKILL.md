---
name: injectable-trait
description: Makes traits injectable using injectable_trait and dyn Trait injection. Use when defining injectable service traits, enabling mock implementations, or using trait objects in DI.
---

# injectable_trait Macro

Generates infrastructure for trait injection, including a type-erased provider and `Inject<dyn Trait>` support.

## Basic pattern

```rust
use injectable::prelude::*;

#[injectable_trait]
pub trait EmailSender {
    async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), std::error::Error>;
}

pub struct SmtpSender;

#[async_trait]
impl EmailSender for SmtpSender {
    async fn send(&self, to: &str, subject: &str, body: &str) -> Result<(), std::error::Error> {
        println!("Sending email to {to}: {subject}");
        Ok(())
    }
}

bind!(dyn EmailSender => SmtpSender);

#[injectable]
struct NotificationService {
    sender: Inject<dyn EmailSender>,
}
```

## How it works

1. `#[injectable_trait]` generates a `Provider` impl for the trait
2. `bind!()` creates a static binding from `dyn Trait` to `Concrete`
3. `Inject<dyn Trait>` resolves via the bound concrete type's provider

## With default implementation

```rust
#[injectable_trait]
pub trait Logger {
    fn log(&self, msg: &str) {
        println!("[LOG] {msg}");
    }
}

pub struct NoOpLogger;

impl Logger for NoOpLogger {}

bind!(dyn Logger => NoOpLogger);
```

## Mocking in tests

```rust
pub struct MockEmailSender;

#[async_trait]
impl EmailSender for MockEmailSender {
    async fn send(&self, to: &str, _subject: &str, _body: &str) -> Result<(), std::error::Error> {
        println!("[MOCK] Would send email to {to}");
        Ok(())
    }
}

// Override binding
bind!(dyn EmailSender => MockEmailSender);
```

## Error handling

```rust
#[injectable]
struct UserService {
    sender: Inject<dyn EmailSender>,
}

impl UserService {
    async fn notify_user(&self, user: &User) -> Result<(), std::error::Error> {
        self.sender
            .send(&user.email, "Welcome!", "Hello from injectable!")
            .await
    }
}
```

## Guidelines

- Use `#[injectable_trait]` on traits that have a single primary implementation
- Use `bind!()` to declare which concrete type satisfies the trait
- `Inject<dyn Trait>` requires both `#[injectable_trait]` and `bind!()`

This pattern enables dependency inversion in your DI graph — depend on abstractions (traits), not concrete types.
