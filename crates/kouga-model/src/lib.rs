//! Small SQLx-backed model operations. Model declarations and attribute generation live in T12.

use std::marker::PhantomData;

use kouga_db::{Acquire, DbError, DbErrorKind, Postgres, QueryBuilder, Transaction};
use sqlx::{Encode, FromRow, Type, postgres::PgRow};

pub use kouga_db::Db;
pub use sqlx::types::Uuid;

/// Implement for application enum/newtype columns; nullable `Option<T>` is deliberately excluded.
pub trait Comparable: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send + 'static {}
impl Comparable for bool {}
impl Comparable for i16 {}
impl Comparable for i32 {}
impl Comparable for i64 {}
impl Comparable for f32 {}
impl Comparable for f64 {}
impl Comparable for String {}
impl Comparable for Uuid {}
impl Comparable for sqlx::types::chrono::NaiveDate {}
impl Comparable for sqlx::types::chrono::NaiveDateTime {}
impl Comparable for sqlx::types::chrono::DateTime<sqlx::types::chrono::Utc> {}
impl Comparable for sqlx::types::Decimal {}

pub trait Model: for<'r> FromRow<'r, PgRow> + Send + Unpin + Sized {
    const TABLE: &'static str;
    const COLUMNS: &'static [&'static str];
}

trait Bind: Send {
    fn push(self: Box<Self>, builder: &mut QueryBuilder<Postgres>);
}

struct Bound<T>(T);

impl<T> Bind for Bound<T>
where
    T: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send + 'static,
{
    fn push(self: Box<Self>, builder: &mut QueryBuilder<Postgres>) {
        builder.push_bind(self.0);
    }
}

fn bound<T>(value: T) -> Box<dyn Bind>
where
    T: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send + 'static,
{
    Box::new(Bound(value))
}

fn invalid() -> DbError {
    DbError::new(DbErrorKind::InvalidInput)
}

fn identifier(name: &str) -> Result<(), DbError> {
    if !name.is_empty()
        && name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
        && !name.as_bytes()[0].is_ascii_digit()
    {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn column<M: Model>(name: &str) -> Result<(), DbError> {
    identifier(name)?;
    if M::COLUMNS.contains(&name) {
        Ok(())
    } else {
        Err(invalid())
    }
}

fn table<M: Model>() -> Result<&'static str, DbError> {
    identifier(M::TABLE)?;
    if !M::COLUMNS.contains(&"id") {
        return Err(invalid());
    }
    Ok(M::TABLE)
}

fn quoted(builder: &mut QueryBuilder<Postgres>, name: &str) {
    builder.push("\"").push(name).push("\"");
}

pub struct Column<M, T> {
    name: &'static str,
    marker: PhantomData<fn() -> (M, T)>,
}

impl<M, T> Copy for Column<M, T> {}
impl<M, T> Clone for Column<M, T> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<M, T> Column<M, T> {
    pub const fn new(name: &'static str) -> Self {
        Self {
            name,
            marker: PhantomData,
        }
    }

    pub fn asc(self) -> Order<M> {
        Order {
            name: self.name,
            descending: false,
            marker: PhantomData,
        }
    }
    pub fn desc(self) -> Order<M> {
        Order {
            name: self.name,
            descending: true,
            marker: PhantomData,
        }
    }
    pub fn is_null(self) -> Predicate<M> {
        Predicate::new(Expr::Null(self.name, true))
    }
    pub fn is_not_null(self) -> Predicate<M> {
        Predicate::new(Expr::Null(self.name, false))
    }
}

impl<M, T> Column<M, T>
where
    T: Comparable,
{
    pub fn eq(self, value: T) -> Predicate<M> {
        Predicate::new(Expr::Compare(self.name, "=", bound(value)))
    }
    pub fn lt(self, value: T) -> Predicate<M> {
        Predicate::new(Expr::Compare(self.name, "<", bound(value)))
    }
    pub fn le(self, value: T) -> Predicate<M> {
        Predicate::new(Expr::Compare(self.name, "<=", bound(value)))
    }
    pub fn gt(self, value: T) -> Predicate<M> {
        Predicate::new(Expr::Compare(self.name, ">", bound(value)))
    }
    pub fn ge(self, value: T) -> Predicate<M> {
        Predicate::new(Expr::Compare(self.name, ">=", bound(value)))
    }
}

impl<M, T: Comparable> Column<M, Option<T>> {
    pub fn eq_value(self, value: T) -> Predicate<M> {
        Predicate::new(Expr::Compare(self.name, "=", bound(value)))
    }
}

impl<M, T> Column<M, T>
where
    Vec<T>: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send + 'static,
{
    pub fn in_list(self, values: Vec<T>) -> Predicate<M> {
        if values.is_empty() {
            Predicate::new(Expr::False)
        } else {
            Predicate::new(Expr::In(self.name, bound(values)))
        }
    }
}

pub struct Predicate<M> {
    expr: Expr,
    marker: PhantomData<M>,
}

enum Expr {
    Compare(&'static str, &'static str, Box<dyn Bind>),
    In(&'static str, Box<dyn Bind>),
    Null(&'static str, bool),
    False,
    Both(Box<Self>, Box<Self>),
    Either(Box<Self>, Box<Self>),
}

impl<M> Predicate<M> {
    fn new(expr: Expr) -> Self {
        Self {
            expr,
            marker: PhantomData,
        }
    }
    pub fn and(self, other: Self) -> Self {
        Self::new(Expr::Both(Box::new(self.expr), Box::new(other.expr)))
    }
    pub fn or(self, other: Self) -> Self {
        Self::new(Expr::Either(Box::new(self.expr), Box::new(other.expr)))
    }
}

impl<M: Model> Predicate<M> {
    fn push(self, builder: &mut QueryBuilder<Postgres>) -> Result<(), DbError> {
        self.expr.push::<M>(builder)
    }
}

impl Expr {
    fn binds(&self) -> usize {
        match self {
            Self::Compare(..) | Self::In(..) => 1,
            Self::Both(a, b) | Self::Either(a, b) => a.binds().saturating_add(b.binds()),
            Self::Null(..) | Self::False => 0,
        }
    }

    fn push<M: Model>(self, builder: &mut QueryBuilder<Postgres>) -> Result<(), DbError> {
        match self {
            Self::Compare(name, op, value) => {
                column::<M>(name)?;
                quoted(builder, name);
                builder.push(" ").push(op).push(" ");
                value.push(builder);
            }
            Self::In(name, value) => {
                column::<M>(name)?;
                quoted(builder, name);
                builder.push(" = ANY(");
                value.push(builder);
                builder.push(")");
            }
            Self::Null(name, yes) => {
                column::<M>(name)?;
                quoted(builder, name);
                builder.push(if yes { " IS NULL" } else { " IS NOT NULL" });
            }
            Self::False => {
                builder.push("FALSE");
            }
            Self::Both(a, b) => {
                builder.push("(");
                a.push::<M>(builder)?;
                builder.push(" AND ");
                b.push::<M>(builder)?;
                builder.push(")");
            }
            Self::Either(a, b) => {
                let op = " OR ";
                builder.push("(");
                a.push::<M>(builder)?;
                builder.push(op);
                b.push::<M>(builder)?;
                builder.push(")");
            }
        }
        Ok(())
    }
}

pub struct Order<M> {
    name: &'static str,
    descending: bool,
    marker: PhantomData<M>,
}

pub struct Query<M> {
    predicate: Option<Predicate<M>>,
    order: Vec<Order<M>>,
    limit: Option<i64>,
    offset: Option<i64>,
}

impl<M: Model> Default for Query<M> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: Model> Query<M> {
    pub fn new() -> Self {
        Self {
            predicate: None,
            order: Vec::new(),
            limit: None,
            offset: None,
        }
    }
    pub fn filter(mut self, predicate: Predicate<M>) -> Self {
        self.predicate = Some(match self.predicate {
            Some(old) => old.and(predicate),
            None => predicate,
        });
        self
    }
    pub fn order_by(mut self, order: Order<M>) -> Self {
        self.order.push(order);
        self
    }
    pub fn limit(mut self, limit: i64) -> Self {
        self.limit = Some(limit);
        self
    }
    pub fn offset(mut self, offset: i64) -> Self {
        self.offset = Some(offset);
        self
    }
    pub fn page(self, page: i64, per_page: i64) -> PageQuery<M> {
        PageQuery {
            query: self,
            page,
            per_page,
        }
    }
    pub fn for_update(self) -> LockedQuery<M> {
        LockedQuery(self)
    }

    fn build(
        self,
        select: &str,
        paged: bool,
        lock: bool,
    ) -> Result<QueryBuilder<Postgres>, DbError> {
        let table = table::<M>()?;
        if self.limit.is_some_and(|n| n < 0) || self.offset.is_some_and(|n| n < 0) {
            return Err(invalid());
        }
        let binds = self.predicate.as_ref().map_or(0, |p| p.expr.binds())
            + usize::from(self.limit.is_some())
            + usize::from(self.offset.is_some());
        if binds >= 65_535 {
            return Err(invalid());
        }
        let mut builder = QueryBuilder::new(select);
        quoted(&mut builder, table);
        if let Some(predicate) = self.predicate {
            builder.push(" WHERE ");
            predicate.push(&mut builder)?;
        }
        if !self.order.is_empty() || paged {
            builder.push(" ORDER BY ");
            let has_id = self.order.iter().any(|o| o.name == "id");
            let had_order = !self.order.is_empty();
            for (i, order) in self.order.into_iter().enumerate() {
                column::<M>(order.name)?;
                if i > 0 {
                    builder.push(", ");
                }
                quoted(&mut builder, order.name);
                builder.push(if order.descending { " DESC" } else { " ASC" });
            }
            if paged && !has_id {
                if had_order {
                    builder.push(", ");
                }
                builder.push("\"id\" ASC");
            }
        }
        if let Some(limit) = self.limit {
            builder.push(" LIMIT ").push_bind(limit);
        }
        if let Some(offset) = self.offset {
            builder.push(" OFFSET ").push_bind(offset);
        }
        if lock {
            builder.push(" FOR UPDATE");
        }
        Ok(builder)
    }

    pub async fn fetch_all<'c, A>(self, db: A) -> Result<Vec<M>, DbError>
    where
        A: Acquire<'c, Database = Postgres> + Send,
    {
        let mut query = self.build("SELECT * FROM ", false, false)?;
        let mut conn = db.acquire().await.map_err(DbError::from)?;
        query
            .build_query_as()
            .fetch_all(&mut *conn)
            .await
            .map_err(Into::into)
    }

    pub async fn fetch_optional<'c, A>(mut self, db: A) -> Result<Option<M>, DbError>
    where
        A: Acquire<'c, Database = Postgres> + Send,
    {
        self.limit = Some(2);
        let rows = self.fetch_all(db).await?;
        if rows.len() > 1 {
            Err(DbError::new(DbErrorKind::Integrity))
        } else {
            Ok(rows.into_iter().next())
        }
    }

    pub async fn count<'c, A>(mut self, db: A) -> Result<i64, DbError>
    where
        A: Acquire<'c, Database = Postgres> + Send,
    {
        self.order.clear();
        self.limit = None;
        self.offset = None;
        let mut query = self.build("SELECT count(*) FROM ", false, false)?;
        let mut conn = db.acquire().await.map_err(DbError::from)?;
        query
            .build_query_scalar()
            .fetch_one(&mut *conn)
            .await
            .map_err(Into::into)
    }

    pub async fn exists<'c, A>(mut self, db: A) -> Result<bool, DbError>
    where
        A: Acquire<'c, Database = Postgres> + Send,
    {
        self.order.clear();
        self.limit = Some(1);
        self.offset = None;
        let mut query = self.build("SELECT 1 FROM ", false, false)?;
        let mut conn = db.acquire().await.map_err(DbError::from)?;
        let found: Option<i32> = query
            .build_query_scalar()
            .fetch_optional(&mut *conn)
            .await?;
        Ok(found.is_some())
    }
}

pub struct LockedQuery<M>(Query<M>);
impl<M: Model> LockedQuery<M> {
    pub async fn fetch_all(self, tx: &mut Transaction<'_>) -> Result<Vec<M>, DbError> {
        let mut query = self.0.build("SELECT * FROM ", false, true)?;
        query
            .build_query_as()
            .fetch_all(&mut **tx)
            .await
            .map_err(Into::into)
    }
}

pub struct PageQuery<M> {
    query: Query<M>,
    page: i64,
    per_page: i64,
}
#[derive(Debug)]
pub struct PageResult<M> {
    pub items: Vec<M>,
    pub page: i64,
    pub per_page: i64,
    pub has_next: bool,
}
impl<M: Model> PageQuery<M> {
    pub async fn fetch<'c, A>(mut self, db: A) -> Result<PageResult<M>, DbError>
    where
        A: Acquire<'c, Database = Postgres> + Send,
    {
        if self.page < 1 || !(1..=100).contains(&self.per_page) {
            return Err(invalid());
        }
        let offset = (self.page - 1)
            .checked_mul(self.per_page)
            .ok_or_else(invalid)?;
        self.query.limit = Some(self.per_page + 1);
        self.query.offset = Some(offset);
        let mut query = self.query.build("SELECT * FROM ", true, false)?;
        let mut conn = db.acquire().await.map_err(DbError::from)?;
        let mut items: Vec<M> = query.build_query_as().fetch_all(&mut *conn).await?;
        let has_next = items.len() > self.per_page as usize;
        items.truncate(self.per_page as usize);
        Ok(PageResult {
            items,
            page: self.page,
            per_page: self.per_page,
            has_next,
        })
    }
}

pub async fn find<'c, M: Model, A>(db: A, id: Uuid) -> Result<Option<M>, DbError>
where
    A: Acquire<'c, Database = Postgres> + Send,
{
    Query::<M>::new()
        .filter(Column::<M, Uuid>::new("id").eq(id))
        .fetch_optional(db)
        .await
}

pub async fn delete<'c, M: Model, A>(db: A, id: Uuid) -> Result<bool, DbError>
where
    A: Acquire<'c, Database = Postgres> + Send,
{
    let mut query = QueryBuilder::<Postgres>::new("DELETE FROM ");
    quoted(&mut query, table::<M>()?);
    query.push(" WHERE \"id\" = ").push_bind(id);
    let mut conn = db.acquire().await.map_err(DbError::from)?;
    Ok(query.build().execute(&mut *conn).await?.rows_affected() == 1)
}

pub struct Field<M> {
    name: &'static str,
    value: Option<Box<dyn Bind>>,
    marker: PhantomData<M>,
}
impl<M> Field<M> {
    pub fn new<T>(column: Column<M, T>, value: T) -> Self
    where
        T: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send + 'static,
    {
        Self {
            name: column.name,
            value: Some(bound(value)),
            marker: PhantomData,
        }
    }
    pub fn null<T>(column: Column<M, Option<T>>) -> Self {
        Self {
            name: column.name,
            value: None,
            marker: PhantomData,
        }
    }
}

fn check_fields<M: Model>(fields: &[Field<M>]) -> Result<(), DbError> {
    if fields.iter().filter(|f| f.value.is_some()).count() >= 65_534 {
        return Err(invalid());
    }
    for (i, field) in fields.iter().enumerate() {
        column::<M>(field.name)?;
        if matches!(field.name, "id" | "created_at" | "updated_at") {
            return Err(invalid());
        }
        if fields[..i].iter().any(|f| f.name == field.name) {
            return Err(invalid());
        }
    }
    Ok(())
}

pub async fn create<'c, M: Model, A>(db: A, id: Uuid, fields: Vec<Field<M>>) -> Result<M, DbError>
where
    A: Acquire<'c, Database = Postgres> + Send,
{
    check_fields::<M>(&fields)?;
    let mut query = QueryBuilder::<Postgres>::new("INSERT INTO ");
    quoted(&mut query, table::<M>()?);
    query.push(" (\"id\"");
    for field in &fields {
        query.push(", ");
        quoted(&mut query, field.name);
    }
    query.push(") VALUES (").push_bind(id);
    for field in fields {
        query.push(", ");
        match field.value {
            Some(value) => value.push(&mut query),
            None => {
                query.push("NULL");
            }
        }
    }
    query.push(") RETURNING *");
    let mut conn = db.acquire().await.map_err(DbError::from)?;
    query
        .build_query_as()
        .fetch_one(&mut *conn)
        .await
        .map_err(Into::into)
}

pub async fn update<'c, M: Model, A>(
    db: A,
    id: Uuid,
    fields: Vec<Field<M>>,
) -> Result<Option<M>, DbError>
where
    A: Acquire<'c, Database = Postgres> + Send,
{
    if fields.is_empty() {
        return Err(invalid());
    }
    check_fields::<M>(&fields)?;
    let mut query = QueryBuilder::<Postgres>::new("UPDATE ");
    quoted(&mut query, table::<M>()?);
    query.push(" SET ");
    for (i, field) in fields.into_iter().enumerate() {
        if i > 0 {
            query.push(", ");
        }
        quoted(&mut query, field.name);
        query.push(" = ");
        match field.value {
            Some(value) => value.push(&mut query),
            None => {
                query.push("NULL");
            }
        }
    }
    if M::COLUMNS.contains(&"updated_at") {
        query.push(", \"updated_at\" = now()");
    }
    query
        .push(" WHERE \"id\" = ")
        .push_bind(id)
        .push(" RETURNING *");
    let mut conn = db.acquire().await.map_err(DbError::from)?;
    query
        .build_query_as()
        .fetch_optional(&mut *conn)
        .await
        .map_err(Into::into)
}
