use sea_query::{
    Alias, Asterisk, Expr, Order, PostgresQueryBuilder,
    Query,
};

// ── filter condition ───────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq)]
pub enum FilterOp {
    Eq, NotEq, Like, Gt, Lt, GtEq, LtEq, IsNull, IsNotNull,
}

impl FilterOp {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Eq        => "=",
            Self::NotEq     => "!=",
            Self::Like      => "LIKE",
            Self::Gt        => ">",
            Self::Lt        => "<",
            Self::GtEq      => ">=",
            Self::LtEq      => "<=",
            Self::IsNull    => "IS NULL",
            Self::IsNotNull => "IS NOT NULL",
        }
    }

    pub fn all() -> Vec<FilterOp> {
        vec![
            Self::Eq, Self::NotEq, Self::Like,
            Self::Gt, Self::Lt, Self::GtEq, Self::LtEq,
            Self::IsNull, Self::IsNotNull,
        ]
    }
}

#[derive(Debug, Clone)]
pub struct WhereClause {
    pub column: String,
    pub op:     FilterOp,
    pub value:  String,  // empty for IS NULL / IS NOT NULL
}

// ── join definition ────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq)]
pub enum JoinType { Inner, Left, Right, Full }

impl JoinType {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Inner => "INNER JOIN",
            Self::Left  => "LEFT JOIN",
            Self::Right => "RIGHT JOIN",
            Self::Full  => "FULL JOIN",
        }
    }
}

#[derive(Debug, Clone)]
pub struct JoinClause {
    pub join_type:  JoinType,
    pub table:      String,
    pub on_left:    String,  // e.g. "orders.user_id"
    pub on_right:   String,  // e.g. "users.id"
}

// ── order-by ───────────────────────────────────────────────────────
#[derive(Debug, Clone, PartialEq)]
pub enum SortDir { Asc, Desc }

#[derive(Debug, Clone)]
pub struct OrderByClause {
    pub column: String,
    pub dir:    SortDir,
}

// ── main builder state (drives the UI panels) ──────────────────────
#[derive(Debug, Clone, Default)]
pub struct QueryBuilder {
    pub schema:   String,
    pub table:    String,
    pub columns:  Vec<String>,    // empty = SELECT *
    pub joins:    Vec<JoinClause>,
    pub filters:  Vec<WhereClause>,
    pub order_by: Vec<OrderByClause>,
    pub limit:    Option<u64>,
    pub offset:   Option<u64>,
}

impl QueryBuilder {
    pub fn new(schema: &str, table: &str) -> Self {
        Self {
            schema: schema.to_string(),
            table:  table.to_string(),
            limit:  Some(1000),   // safe default
            ..Default::default()
        }
    }

    pub fn select_columns(&mut self, cols: Vec<String>) -> &mut Self {
        self.columns = cols; self
    }

    pub fn add_filter(&mut self, f: WhereClause) -> &mut Self {
        self.filters.push(f); self
    }

    pub fn add_join(&mut self, j: JoinClause) -> &mut Self {
        self.joins.push(j); self
    }

    pub fn add_order(&mut self, o: OrderByClause) -> &mut Self {
        self.order_by.push(o); self
    }

    pub fn set_limit(&mut self, n: u64) -> &mut Self {
        self.limit = Some(n); self
    }

    pub fn set_offset(&mut self, n: u64) -> &mut Self {
        self.offset = Some(n); self
    }

    // ── build → SQL string ─────────────────────────────────────────
    pub fn to_sql(&self) -> String {
        let mut q = Query::select();

        if self.columns.is_empty() {
            q.expr(Expr::col(Asterisk));
        } else {
            for col in &self.columns {
                q.expr(Expr::col((Alias::new(&self.table), Alias::new(col))));
            }
        }

        q.from((Alias::new(&self.schema), Alias::new(&self.table)));

        for j in &self.joins {
            let jt = Alias::new(&j.table);
            let (l_tbl, l_col) = split_dot(&j.on_left);
            let (r_tbl, r_col) = split_dot(&j.on_right);
            let cond = Expr::col((Alias::new(l_tbl), Alias::new(l_col)))
                .equals((Alias::new(r_tbl), Alias::new(r_col)));
            match j.join_type {
                JoinType::Inner => { q.inner_join(jt, cond); }
                JoinType::Left  => { q.left_join(jt, cond); }
                JoinType::Right => { q.right_join(jt, cond); }
                JoinType::Full  => { q.full_outer_join(jt, cond); }
            }
        }

        for f in &self.filters {
            let col = Expr::col(Alias::new(&f.column));
            let cond = match f.op {
                FilterOp::Eq        => col.eq(&*f.value),
                FilterOp::NotEq     => col.ne(&*f.value),
                FilterOp::Like      => col.like(&*f.value),
                FilterOp::Gt        => col.gt(&*f.value),
                FilterOp::Lt        => col.lt(&*f.value),
                FilterOp::GtEq      => col.gte(&*f.value),
                FilterOp::LtEq      => col.lte(&*f.value),
                FilterOp::IsNull    => col.is_null(),
                FilterOp::IsNotNull => col.is_not_null(),
            };
            q.and_where(cond);
        }

        for o in &self.order_by {
            let dir = if o.dir == SortDir::Asc { Order::Asc } else { Order::Desc };
            q.order_by(Alias::new(&o.column), dir);
        }

        if let Some(n) = self.limit  { q.limit(n); }
        if let Some(n) = self.offset { q.offset(n); }

        q.to_string(PostgresQueryBuilder)
    }

    pub fn reset(&mut self) {
        self.columns.clear();
        self.joins.clear();
        self.filters.clear();
        self.order_by.clear();
        self.limit  = Some(1000);
        self.offset = None;
    }
}

// ── helper: "table.column" → ("table", "column") ──────────────────
fn split_dot(s: &str) -> (&str, &str) {
    s.split_once('.').unwrap_or(("", s))
}
