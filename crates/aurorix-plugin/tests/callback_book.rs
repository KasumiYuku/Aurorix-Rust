use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use aurorix_core::{Scene, Target};
use aurorix_plugin::{
    CallbackBook, CallbackFuture, CallbackHandler, CallbackRequest, Outgoing, ReplyKind,
};

fn handler(hits: Arc<AtomicUsize>) -> CallbackHandler {
    Arc::new(move |request: CallbackRequest| {
        let hits = Arc::clone(&hits);
        let future: CallbackFuture = Box::pin(async move {
            hits.fetch_add(1, Ordering::SeqCst);
            Some(Outgoing::new(
                ReplyKind::Handler,
                format!("{}|{}|{}", request.button_id, request.data, request.user),
            ))
        });
        future
    })
}

fn request(book: &CallbackBook, id: &str, data: &str) -> CallbackRequest {
    CallbackRequest {
        button_id: id.to_string(),
        data: data.to_string(),
        plugin: book.plugin_of(id).unwrap_or_default(),
        user: String::from("u1"),
        scene: Scene::Group,
        target: Target::new(Scene::Group, "g1").with_user("u1"),
        sender: None,
        interaction_id: String::from("int-1"),
    }
}

#[test]
fn register_reports_duplicates_and_keeps_ids_sorted() {
    let book = CallbackBook::new();
    let hits = Arc::new(AtomicUsize::new(0));
    assert!(book.is_empty());
    assert!(book.register("arcaea.score.best50", handler(Arc::clone(&hits))));
    assert!(!book.register("arcaea.score.best50", handler(Arc::clone(&hits))));
    assert!(book.register("arcaea.common.menu", handler(Arc::clone(&hits))));
    assert_eq!(book.len(), 2);
    assert!(book.contains("arcaea.score.best50"));
    assert!(!book.contains("arcaea.absent"));
    assert_eq!(
        book.ids(),
        vec![
            String::from("arcaea.common.menu"),
            String::from("arcaea.score.best50")
        ]
    );
    assert_eq!(
        book.plugin_of("arcaea.score.best50").as_deref(),
        Some("arcaea")
    );
    assert_eq!(book.plugin_of("arcaea.absent"), None);
}

#[test]
fn scoped_registration_keeps_the_declared_plugin() {
    let book = CallbackBook::new();
    let hits = Arc::new(AtomicUsize::new(0));
    assert!(book.register_scoped(
        "cn.arcaea.score.best50",
        "arcaea",
        handler(Arc::clone(&hits))
    ));
    assert_eq!(
        book.plugin_of("cn.arcaea.score.best50").as_deref(),
        Some("arcaea")
    );
    assert!(book.remove("cn.arcaea.score.best50"));
    assert!(!book.remove("cn.arcaea.score.best50"));
    assert!(book.is_empty());
}

#[tokio::test]
async fn dispatch_runs_the_registered_handler_only() {
    let book = CallbackBook::new();
    let hits = Arc::new(AtomicUsize::new(0));
    book.register("arcaea.score.best50", handler(Arc::clone(&hits)));
    assert!(book.dispatch(request(&book, "arcaea.absent", "x")).is_none());
    assert_eq!(hits.load(Ordering::SeqCst), 0);
    let future = book
        .dispatch(request(&book, "arcaea.score.best50", "50"))
        .expect("命中注册表必须给出 future");
    let reply = future.await.expect("handler 必须给出回复");
    assert_eq!(reply.body(), "arcaea.score.best50|50|u1");
    assert_eq!(hits.load(Ordering::SeqCst), 1);
    assert_eq!(book.plugin_of("arcaea.score.best50").as_deref(), Some("arcaea"));
}
