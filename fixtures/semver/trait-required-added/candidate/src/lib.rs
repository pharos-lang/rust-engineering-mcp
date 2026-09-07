pub trait Service {
    fn existing(&self);
    fn added(&self);
}
