---
name: bind-macro
description: Binds trait objects to concrete implementations using bind!(). Use when injecting dyn Trait types, swapping implementations (mock vs production), or getting 'conflicting implementations' errors.
---

# bind! Macro

Creates a static binding from a trait to a concrete type for `Inject<dyn Trait>`.

## Basic pattern

```rust
use injectable::prelude::*;

// Define the trait
#[injectable_trait]
pub trait EmailSender {
    async fn send(&self, to: &str, subject: &str, body: &str);
}

// Implement the trait
pub struct SmtpSender;

#[async_trait]
impl EmailSender for SmtpSender {
    async fn send(&self, to: &str, subject: &str, body: &str) {
        println!("Sending email to {to}: {subject}");
    }
}

// Create a singleton binding
bind!(dyn EmailSender => SmtpSender);

// Now Inject<dyn EmailSender> works
#[injectable]
struct NotificationService {
    sender: Inject<dyn EmailSender>,
}
```

## With DynProvider for external traits

```rust
use injectable::prelude::*;

#[injectable_trait]
pub trait Cache {
    fn get(&self, key: &str) -> Option<String>;
    fn set(&self, key: &str, value: String);
}

pub struct InMemoryCache {
    store: std::collections::HashMap<String, String>,
}

impl Cache for InMemoryCache {
    fn get(&self, key: &str) -> Option<String> {
        self.store.get(key).cloned()
    }
    fn set(&self, key: &str, value: String) {
        self.store.insert(key, value);
    }
}

bind!(dyn Cache => InMemoryCache);

#[injectable]
struct UserService {
    #[inject]
    cache: Inject<dyn Cache>,
}
```

## Mock in tests

```rust
pub struct MockEmailSender;

#[async_trait]
impl EmailSender for MockEmailSender {
    async fn send(&self, to: &str, _subject: &str, _body: &str) {
        println!("[MOCK] Would send email to {to}");
    }
}

// Override binding in test
bind!(dyn EmailSender => MockEmailSender);
```

## bind! vs manual registration

| Approach | Use when |
|---|---|
| `bind!(dyn Trait => Concrete)` | Concrete type is `#[injectable]` |
| `DynProvider` | External type or needs custom factory |
| Both combined | Trait from external crate + custom factory |

The `bind!` macro generates the `Extract` impl for `Inject<dyn Trait>` that delegates to `Concrete::Provider`.

See [guides/04-external-types.md](../../guides/04-external-types.md) for more on external types.
