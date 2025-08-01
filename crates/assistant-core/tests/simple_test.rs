#[test]
fn test_simple() {
    assert_eq!(2 + 2, 4);
}

#[tokio::test]
async fn test_async_simple() {
    let result = async { 42 }.await;
    assert_eq!(result, 42);
}