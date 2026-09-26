//! Small SQLx-backed model operations. Model declarations and attribute generation live in T12.

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    marker::PhantomData,
};

use kouga_db::{Acquire, DbError, DbErrorKind, PgConnection, Postgres, QueryBuilder, Transaction};
use sqlx::{Encode, FromRow, Type, postgres::PgRow};
use tracing::Instrument;

pub use kouga_core as core;
pub use kouga_db as db;
pub use kouga_db::Db;
/// Derive a DB model from named fields. The table name is explicit; UUID `id` is required.
///
/// ```compile_fail
/// #[derive(kouga_model::Model)]
/// #[model(table = "items")]
/// struct WrongId { id: i32 }
/// ```
///
/// `crud_visibility = "private"` keeps generated CRUD inside the model's module.
/// ```compile_fail
/// mod domain {
///     #[derive(kouga_model::Model)]
///     #[model(table = "secrets", crud_visibility = "private")]
///     pub struct Secret { pub id: kouga_model::Uuid, pub name: String }
/// }
/// let _ = domain::Secret::query();
/// ```
pub use kouga_model_derive::Model;
pub use sqlx;
pub use uuid::Uuid;

/// Implement for application enum/newtype columns; nullable `Option<T>` is deliberately excluded.
pub trait Comparable:
    for<'q> Encode<'q, Postgres> + Type<Postgres> + Send + Clone + 'static
{
}
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
    fn id(&self) -> Uuid;
}

trait Bind: Send {
    fn push(self: Box<Self>, builder: &mut QueryBuilder<Postgres>);
    fn clone_box(&self) -> Box<dyn Bind>;
}

impl Clone for Box<dyn Bind> {
    fn clone(&self) -> Self {
        self.clone_box()
    }
}

struct Bound<T>(T);

impl<T> Bind for Bound<T>
where
    T: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send + Clone + 'static,
{
    fn push(self: Box<Self>, builder: &mut QueryBuilder<Postgres>) {
        builder.push_bind(self.0);
    }
    fn clone_box(&self) -> Box<dyn Bind> {
        Box::new(Bound(self.0.clone()))
    }
}

fn bound<T>(value: T) -> Box<dyn Bind>
where
    T: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send + Clone + 'static,
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
    Vec<T>: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send + Clone + 'static,
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
impl<M> Clone for Predicate<M> {
    fn clone(&self) -> Self {
        Self {
            expr: self.expr.clone(),
            marker: PhantomData,
        }
    }
}

#[derive(Clone)]
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
impl<M> Clone for Order<M> {
    fn clone(&self) -> Self {
        Self {
            name: self.name,
            descending: self.descending,
            marker: PhantomData,
        }
    }
}

pub struct Query<M> {
    predicate: Option<Predicate<M>>,
    order: Vec<Order<M>>,
    limit: Option<i64>,
    offset: Option<i64>,
}
impl<M> Clone for Query<M> {
    fn clone(&self) -> Self {
        Self {
            predicate: self.predicate.clone(),
            order: self.order.clone(),
            limit: self.limit,
            offset: self.offset,
        }
    }
}

/// A model and its explicitly fetched relation. Plain models never issue hidden queries.
#[derive(Debug)]
pub struct Loaded<M, R> {
    pub model: M,
    pub related: R,
}

pub trait Relation<M: Model>: Send {
    type Related: Send;
    fn load(
        self,
        parents: &[M],
        db: &mut PgConnection,
    ) -> impl std::future::Future<Output = Result<Vec<Self::Related>, DbError>> + Send;
}

macro_rules! tuple_relation {
    ($($name:ident : $index:tt),+) => {
        #[allow(non_snake_case)]
        impl<M: Model + Sync, $($name: Relation<M>,)+> Relation<M> for ($($name,)+) {
            type Related = ($($name::Related,)+);
            async fn load(self, parents: &[M], db: &mut PgConnection) -> Result<Vec<Self::Related>, DbError> {
                $(let $name = self.$index.load(parents, &mut *db).await?;)+
                let mut rows = parents.iter().map(|_| ());
                $(let mut $name = $name.into_iter();)+
                Ok(rows.by_ref().map(|_| ($($name.next().expect("relation cardinality"),)+)).collect())
            }
        }
    };
}
tuple_relation!(A:0, B:1);
tuple_relation!(A:0, B:1, C:2);
tuple_relation!(A:0, B:1, C:2, D:3);

pub struct PreloadQuery<M, R> {
    query: Query<M>,
    relation: R,
}

pub struct PreloadPageQuery<M, R> {
    page: PageQuery<M>,
    relation: R,
}

impl<M: Model, R: Relation<M>> PreloadQuery<M, R> {
    pub fn page(self, page: i64, per_page: i64) -> PreloadPageQuery<M, R> {
        PreloadPageQuery {
            page: self.query.page(page, per_page),
            relation: self.relation,
        }
    }
    pub fn limit(mut self, limit: i64) -> Self {
        self.query = self.query.limit(limit);
        self
    }
    pub fn offset(mut self, offset: i64) -> Self {
        self.query = self.query.offset(offset);
        self
    }
    pub fn filter(mut self, predicate: Predicate<M>) -> Self {
        self.query = self.query.filter(predicate);
        self
    }
    pub fn order_by(mut self, order: Order<M>) -> Self {
        self.query = self.query.order_by(order);
        self
    }

    pub fn fetch_all<'a, 'c, A>(
        self,
        db: A,
    ) -> impl Future<Output = Result<Vec<Loaded<M, R::Related>>, DbError>> + Send + 'a
    where
        A: Acquire<'c, Database = Postgres> + Send + 'a,
        M: 'a,
        R: 'a,
    {
        let span = tracing::info_span!(
            "kouga.db.query",
            db.operation = "preload",
            db.table = M::TABLE
        );
        async move {
            let mut conn = db.acquire().await.map_err(DbError::from)?;
            let parents = self.query.fetch_all_conn(&mut conn).await?;
            let related = self.relation.load(&parents, &mut conn).await?;
            Ok(parents
                .into_iter()
                .zip(related)
                .map(|(model, related)| Loaded { model, related })
                .collect())
        }
        .instrument(span)
    }
}

impl<M: Model, R: Relation<M>> PreloadPageQuery<M, R> {
    pub fn fetch<'a, 'c, A>(
        self,
        db: A,
    ) -> impl Future<Output = Result<PageResult<Loaded<M, R::Related>>, DbError>> + Send + 'a
    where
        A: Acquire<'c, Database = Postgres> + Send + 'a,
        M: 'a,
        R: 'a,
    {
        let span = tracing::info_span!(
            "kouga.db.query",
            db.operation = "preload_page",
            db.table = M::TABLE
        );
        async move {
            let mut conn = db.acquire().await.map_err(DbError::from)?;
            let page = self.page.fetch_conn(&mut conn).await?;
            let related = self.relation.load(&page.items, &mut conn).await?;
            Ok(PageResult {
                items: page
                    .items
                    .into_iter()
                    .zip(related)
                    .map(|(model, related)| Loaded { model, related })
                    .collect(),
                page: page.page,
                per_page: page.per_page,
                has_next: page.has_next,
            })
        }
        .instrument(span)
    }
}

/// A typed relation descriptor; explicit UUID keys avoid runtime schema guessing.
pub struct BelongsTo<M, R> {
    key: fn(&M) -> Option<Uuid>,
    query: Query<R>,
}

pub struct OptionalBelongsTo<M, R>(BelongsTo<M, R>);

pub struct HasMany<M, R> {
    key: fn(&R) -> Option<Uuid>,
    column: &'static str,
    query: Query<R>,
    marker: PhantomData<M>,
}

pub struct HasOne<M, R>(HasMany<M, R>);

pub struct ManyToMany<M, J, R> {
    parent_key: fn(&J) -> Option<Uuid>,
    related_key: fn(&J) -> Option<Uuid>,
    parent_column: &'static str,
    related: Query<R>,
    marker: PhantomData<M>,
}

impl<M: Model, R: Model> BelongsTo<M, R> {
    pub fn new(key: fn(&M) -> Option<Uuid>) -> Self {
        Self {
            key,
            query: Query::new(),
        }
    }
    pub fn filter(mut self, predicate: Predicate<R>) -> Self {
        self.query = self.query.filter(predicate);
        self
    }
    pub fn order_by(mut self, order: Order<R>) -> Self {
        self.query = self.query.order_by(order);
        self
    }
    pub fn preload<N: Relation<R>>(self, nested: N) -> Nested<M, R, N> {
        Nested {
            outer: self,
            nested,
        }
    }
}

impl<M: Model, R: Model> OptionalBelongsTo<M, R> {
    pub fn new(key: fn(&M) -> Option<Uuid>) -> Self {
        Self(BelongsTo::new(key))
    }
    pub fn filter(mut self, predicate: Predicate<R>) -> Self {
        self.0 = self.0.filter(predicate);
        self
    }
}

impl<M: Model, R: Model> HasMany<M, R> {
    pub fn new(column: &'static str, key: fn(&R) -> Option<Uuid>) -> Self {
        Self {
            key,
            column,
            query: Query::new(),
            marker: PhantomData,
        }
    }
    pub fn filter(mut self, predicate: Predicate<R>) -> Self {
        self.query = self.query.filter(predicate);
        self
    }
    pub fn order_by(mut self, order: Order<R>) -> Self {
        self.query = self.query.order_by(order);
        self
    }
}

impl<M: Model, R: Model> HasOne<M, R> {
    pub fn new(column: &'static str, key: fn(&R) -> Option<Uuid>) -> Self {
        Self(HasMany::new(column, key))
    }
    pub fn filter(mut self, predicate: Predicate<R>) -> Self {
        self.0 = self.0.filter(predicate);
        self
    }
}

impl<M: Model, J: Model, R: Model> ManyToMany<M, J, R> {
    pub fn new(
        parent_column: &'static str,
        parent_key: fn(&J) -> Option<Uuid>,
        related_key: fn(&J) -> Option<Uuid>,
    ) -> Self {
        Self {
            parent_key,
            related_key,
            parent_column,
            related: Query::new(),
            marker: PhantomData,
        }
    }
    pub fn filter(mut self, predicate: Predicate<R>) -> Self {
        self.related = self.related.filter(predicate);
        self
    }
    pub fn order_by(mut self, order: Order<R>) -> Self {
        self.related = self.related.order_by(order);
        self
    }
}

pub struct Nested<M, R, N> {
    outer: BelongsTo<M, R>,
    nested: N,
}

async fn matching<R: Model>(
    query: Query<R>,
    column_name: &'static str,
    mut keys: Vec<Uuid>,
    db: &mut PgConnection,
) -> Result<Vec<R>, DbError> {
    if keys.is_empty() {
        return Ok(Vec::new());
    }
    let mut seen = HashSet::new();
    keys.retain(|id| seen.insert(*id));
    let mut rows = Vec::new();
    for chunk in keys.chunks(1_000) {
        let scoped = query
            .clone()
            .filter(Predicate::new(Expr::In(column_name, bound(chunk.to_vec()))));
        rows.extend(scoped.fetch_all_conn(&mut *db).await?);
    }
    Ok(rows)
}

impl<M: Model + Sync, R: Model + Clone> Relation<M> for BelongsTo<M, R> {
    type Related = R;
    async fn load(self, parents: &[M], db: &mut PgConnection) -> Result<Vec<R>, DbError> {
        let keys = parents.iter().filter_map(self.key).collect();
        let rows = matching(self.query, "id", keys, db).await?;
        let by_id: HashMap<Uuid, R> = rows.into_iter().map(|row| (row.id(), row)).collect();
        parents
            .iter()
            .map(|parent| {
                (self.key)(parent)
                    .and_then(|id| by_id.get(&id))
                    .cloned()
                    .ok_or_else(|| DbError::new(DbErrorKind::Integrity))
            })
            .collect()
    }
}

impl<M: Model + Sync, R: Model + Clone> Relation<M> for OptionalBelongsTo<M, R> {
    type Related = Option<R>;
    async fn load(self, parents: &[M], db: &mut PgConnection) -> Result<Vec<Option<R>>, DbError> {
        let keys = parents.iter().filter_map(self.0.key).collect();
        let rows = matching(self.0.query, "id", keys, db).await?;
        let by_id: HashMap<Uuid, R> = rows.into_iter().map(|row| (row.id(), row)).collect();
        Ok(parents
            .iter()
            .map(|parent| (self.0.key)(parent).and_then(|id| by_id.get(&id)).cloned())
            .collect())
    }
}

impl<M: Model + Sync, R: Model> Relation<M> for HasMany<M, R> {
    type Related = Vec<R>;
    async fn load(self, parents: &[M], db: &mut PgConnection) -> Result<Vec<Vec<R>>, DbError> {
        let keys: Vec<_> = parents.iter().map(Model::id).collect();
        let rows = matching(self.query, self.column, keys, db).await?;
        let mut grouped: HashMap<Uuid, Vec<R>> = HashMap::new();
        for row in rows {
            if let Some(id) = (self.key)(&row) {
                grouped.entry(id).or_default().push(row);
            }
        }
        Ok(parents
            .iter()
            .map(|parent| grouped.remove(&parent.id()).unwrap_or_default())
            .collect())
    }
}

impl<M: Model + Sync, R: Model> Relation<M> for HasOne<M, R> {
    type Related = Option<R>;
    async fn load(self, parents: &[M], db: &mut PgConnection) -> Result<Vec<Option<R>>, DbError> {
        let groups: Vec<Vec<R>> = self.0.load(parents, db).await?;
        groups
            .into_iter()
            .map(|mut group| {
                if group.len() > 1 {
                    Err(DbError::new(DbErrorKind::Integrity))
                } else {
                    Ok(group.pop())
                }
            })
            .collect()
    }
}

impl<M: Model + Sync, J: Model, R: Model + Clone> Relation<M> for ManyToMany<M, J, R> {
    type Related = Vec<R>;
    async fn load(self, parents: &[M], db: &mut PgConnection) -> Result<Vec<Vec<R>>, DbError> {
        let links = matching(
            Query::<J>::new(),
            self.parent_column,
            parents.iter().map(Model::id).collect(),
            &mut *db,
        )
        .await?;
        let target_ids: Vec<_> = links.iter().filter_map(self.related_key).collect();
        // ponytail: reject cross-chunk ordered many-to-many; add a merge comparator if this limit matters.
        if !self.related.order.is_empty() && target_ids.iter().collect::<HashSet<_>>().len() > 1_000
        {
            return Err(invalid());
        }
        let related = matching(self.related, "id", target_ids, db).await?;
        let mut owners: HashMap<Uuid, Vec<Uuid>> = HashMap::new();
        for link in links {
            if let (Some(parent), Some(target)) =
                ((self.parent_key)(&link), (self.related_key)(&link))
            {
                owners.entry(target).or_default().push(parent);
            }
        }
        let mut grouped: HashMap<Uuid, Vec<R>> = HashMap::new();
        for row in related {
            if let Some(parent_ids) = owners.get(&row.id()) {
                for parent in parent_ids {
                    grouped.entry(*parent).or_default().push(row.clone());
                }
            }
        }
        Ok(parents
            .iter()
            .map(|parent| grouped.remove(&parent.id()).unwrap_or_default())
            .collect())
    }
}

impl<M: Model + Sync, R: Model + Clone + Sync, N: Relation<R> + Send> Relation<M>
    for Nested<M, R, N>
{
    type Related = Loaded<R, N::Related>;
    async fn load(
        self,
        parents: &[M],
        db: &mut PgConnection,
    ) -> Result<Vec<Self::Related>, DbError> {
        let outer: Vec<R> = self.outer.load(parents, &mut *db).await?;
        let inner = self.nested.load(&outer, db).await?;
        Ok(outer
            .into_iter()
            .zip(inner)
            .map(|(model, related)| Loaded { model, related })
            .collect())
    }
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
    pub fn filter_uuid_column(self, column: &'static str, id: Uuid) -> Self {
        self.filter(Predicate::new(Expr::Compare(column, "=", bound(id))))
    }
    pub fn filter_false(self) -> Self {
        self.filter(Predicate::new(Expr::False))
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
    pub fn preload<R: Relation<M>>(self, relation: R) -> PreloadQuery<M, R> {
        PreloadQuery {
            query: self,
            relation,
        }
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

    pub fn fetch_all<'a, 'c, A>(
        self,
        db: A,
    ) -> impl Future<Output = Result<Vec<M>, DbError>> + Send + 'a
    where
        A: Acquire<'c, Database = Postgres> + Send + 'a,
        M: 'a,
    {
        let span = tracing::info_span!(
            "kouga.db.query",
            db.operation = "select",
            db.table = M::TABLE
        );
        async move {
            let mut query = self.build("SELECT * FROM ", false, false)?;
            let mut conn = db.acquire().await.map_err(DbError::from)?;
            query
                .build_query_as()
                .fetch_all(&mut *conn)
                .await
                .map_err(Into::into)
        }
        .instrument(span)
    }

    async fn fetch_all_conn(self, db: &mut PgConnection) -> Result<Vec<M>, DbError> {
        let mut query = self.build("SELECT * FROM ", false, false)?;
        query
            .build_query_as()
            .fetch_all(db)
            .await
            .map_err(Into::into)
    }

    // SQLx Acquire needs an explicit Send future for async HTTP handlers (rust-lang #100013).
    #[allow(clippy::manual_async_fn)]
    pub fn fetch_optional<'a, 'c, A>(
        mut self,
        db: A,
    ) -> impl Future<Output = Result<Option<M>, DbError>> + Send + 'a
    where
        A: Acquire<'c, Database = Postgres> + Send + 'a,
        M: 'a,
    {
        async move {
            self.limit = Some(2);
            let rows = self.fetch_all(db).await?;
            if rows.len() > 1 {
                Err(DbError::new(DbErrorKind::Integrity))
            } else {
                Ok(rows.into_iter().next())
            }
        }
    }

    pub fn count<'a, 'c, A>(
        mut self,
        db: A,
    ) -> impl Future<Output = Result<i64, DbError>> + Send + 'a
    where
        A: Acquire<'c, Database = Postgres> + Send + 'a,
        M: 'a,
    {
        let span = tracing::info_span!(
            "kouga.db.query",
            db.operation = "count",
            db.table = M::TABLE
        );
        async move {
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
        .instrument(span)
    }

    pub fn exists<'a, 'c, A>(
        mut self,
        db: A,
    ) -> impl Future<Output = Result<bool, DbError>> + Send + 'a
    where
        A: Acquire<'c, Database = Postgres> + Send + 'a,
        M: 'a,
    {
        let span = tracing::info_span!(
            "kouga.db.query",
            db.operation = "exists",
            db.table = M::TABLE
        );
        async move {
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
        .instrument(span)
    }
}

pub struct LockedQuery<M>(Query<M>);
impl<M: Model> LockedQuery<M> {
    #[tracing::instrument(name = "kouga.db.query", skip_all, fields(db.operation = "select_for_update", db.table = M::TABLE))]
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
    pub fn fetch<'a, 'c, A>(
        self,
        db: A,
    ) -> impl Future<Output = Result<PageResult<M>, DbError>> + Send + 'a
    where
        A: Acquire<'c, Database = Postgres> + Send + 'a,
        M: 'a,
    {
        let span =
            tracing::info_span!("kouga.db.query", db.operation = "page", db.table = M::TABLE);
        async move {
            let mut conn = db.acquire().await.map_err(DbError::from)?;
            self.fetch_conn(&mut conn).await
        }
        .instrument(span)
    }
    async fn fetch_conn(mut self, conn: &mut PgConnection) -> Result<PageResult<M>, DbError> {
        if self.page < 1 || !(1..=100).contains(&self.per_page) {
            return Err(invalid());
        }
        let offset = (self.page - 1)
            .checked_mul(self.per_page)
            .ok_or_else(invalid)?;
        self.query.limit = Some(self.per_page + 1);
        self.query.offset = Some(offset);
        let mut query = self.query.build("SELECT * FROM ", true, false)?;
        let mut items: Vec<M> = query.build_query_as().fetch_all(conn).await?;
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

pub fn find<'a, 'c, M: Model + 'a, A>(
    db: A,
    id: Uuid,
) -> impl Future<Output = Result<Option<M>, DbError>> + Send + 'a
where
    A: Acquire<'c, Database = Postgres> + Send + 'a,
{
    Query::<M>::new()
        .filter(Column::<M, Uuid>::new("id").eq(id))
        .fetch_optional(db)
}

pub fn delete<'a, 'c, M: Model + 'a, A>(
    db: A,
    id: Uuid,
) -> impl Future<Output = Result<bool, DbError>> + Send + 'a
where
    A: Acquire<'c, Database = Postgres> + Send + 'a,
{
    let span = tracing::info_span!(
        "kouga.db.query",
        db.operation = "delete",
        db.table = M::TABLE
    );
    async move {
        let mut query = QueryBuilder::<Postgres>::new("DELETE FROM ");
        quoted(&mut query, table::<M>()?);
        query.push(" WHERE \"id\" = ").push_bind(id);
        let mut conn = db.acquire().await.map_err(DbError::from)?;
        Ok(query.build().execute(&mut *conn).await?.rows_affected() == 1)
    }
    .instrument(span)
}

pub struct Field<M> {
    name: &'static str,
    value: Option<Box<dyn Bind>>,
    marker: PhantomData<M>,
}
impl<M> Field<M> {
    pub fn new<T>(column: Column<M, T>, value: T) -> Self
    where
        T: for<'q> Encode<'q, Postgres> + Type<Postgres> + Send + Clone + 'static,
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

pub fn create<'a, 'c, M: Model + 'a, A>(
    db: A,
    id: Uuid,
    fields: Vec<Field<M>>,
) -> impl Future<Output = Result<M, DbError>> + Send + 'a
where
    A: Acquire<'c, Database = Postgres> + Send + 'a,
{
    let span = tracing::info_span!(
        "kouga.db.query",
        db.operation = "insert",
        db.table = M::TABLE
    );
    async move {
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
    .instrument(span)
}

pub fn update<'a, 'c, M: Model + 'a, A>(
    db: A,
    id: Uuid,
    fields: Vec<Field<M>>,
) -> impl Future<Output = Result<Option<M>, DbError>> + Send + 'a
where
    A: Acquire<'c, Database = Postgres> + Send + 'a,
{
    let span = tracing::info_span!(
        "kouga.db.query",
        db.operation = "update",
        db.table = M::TABLE
    );
    async move {
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
    .instrument(span)
}
