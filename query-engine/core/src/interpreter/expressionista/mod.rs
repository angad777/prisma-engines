use super::{expression::*, Env, InterpretationResult, InterpreterError};
use crate::{query_graph::*, Query, WriteQuery};
use prisma_models::ModelProjection;
use std::convert::TryInto;

pub struct Expressionista;

// WIP Assumptions:
// - Result node is always found on the first or second level of the query graph -> We can simply order the result node to the back
// - The dependencies expressed in the graph guarantee that whatever expression is returned from translating a node does indeed return the necessary
//   data as a result of the entire expression evaluation.
impl Expressionista {
    pub fn translate(mut graph: QueryGraph) -> InterpretationResult<Expression> {
        let root_node = graph.root_node();
        Self::translate_node(&mut graph, &root_node)
    }

    fn translate_node(graph: &mut QueryGraph, node: &NodeRef) -> InterpretationResult<Expression> {
        let node_id = node.id();
        let query = graph.node_content(node);
        let outgoing_edges = graph.outgoing_edges(node);
        let incoming_edges = graph.incoming_edges(node);

        let transformers: Vec<_> = Self::collect_query_transformers(graph, &incoming_edges);
        let expected_return_projection = Self::collect_expected_projection(graph, &outgoing_edges);
        let query_expr = Self::query_transform_expression(query.clone(), transformers);

        Ok(Self::translate_query(query))
    }

    fn translate_query(query: &Query) -> Expression {
        match query {
            Query::Write(WriteQuery::CreateRecord(cr)) => translate_create_record(),
            _ => todo!(),
        }
    }

    fn translate_create_record(graph: &QueryGraph) -> Expression {
        // A simple create is straight-forward, not much to do around it.
    }

    // WIP signature
    fn reload_records_conditionally(
        graph: &QueryGraph,
        node_id: String,
        query: &Query,
        query_expr: Expression,
        child_expr: Option<Expression>,
        expected_return_projection: Option<ModelProjection>,
    ) -> Expression {
        match (expected_return_projection, child_expr) {
            // Reload necessary, dependent queries present.
            (Some(p), Some(ce)) if !query.returns(&p) => {
                let reload_query_expr = Self::derive_reload_query_expression(query, node_id.clone(), p);

                Expression::Let {
                    bindings: vec![Binding {
                        name: node_id.clone(), // todo use query name? Or maybe separate meta info.
                        expr: query_expr,
                    }],
                    inner: Box::new(Expression::Let {
                        bindings: vec![Binding {
                            name: node_id,
                            expr: reload_query_expr,
                        }],
                        inner: Box::new(ce),
                    }),
                }
            }

            // Reload necessary, no dependent queries present.
            (Some(p), None) if !query.returns(&p) => {
                let reload_query_expr = Self::derive_reload_query_expression(query, node_id.clone(), p);

                Expression::Let {
                    bindings: vec![Binding {
                        name: node_id.clone(),
                        expr: query_expr,
                    }],
                    inner: Box::new(reload_query_expr),
                }
            }

            // No reload necessary, dependent children present.
            (_, Some(ce)) => Expression::Let {
                bindings: vec![Binding {
                    name: node_id.clone(),
                    expr: query_expr,
                }],
                inner: Box::new(ce),
            },

            // No reload necessary, no dependent children present.
            (_, None) => query_expr,
        }
    }

    fn collect_query_transformers(graph: &QueryGraph, incoming_edges: &[EdgeRef]) -> Vec<QueryTransformer> {
        incoming_edges
            .into_iter()
            .filter_map(|parent_edge| {
                let dep = graph.edge_content(parent_edge);
                let parent_node = graph.edge_source(parent_edge);

                match dep {
                    Some(QueryDependency::InjectFilter(f)) => {
                        Some(QueryTransformer::InjectFilter(f.clone().into(), parent_node.id()))
                    }
                    Some(QueryDependency::InjectData(i)) => {
                        Some(QueryTransformer::InjectData(i.clone().into(), parent_node.id()))
                    }
                    None => None,
                }
            })
            .collect()
    }

    fn collect_expected_projection(graph: &QueryGraph, outgoing_edges: &[EdgeRef]) -> Option<ModelProjection> {
        if outgoing_edges.is_empty() {
            return None;
        }

        let expected_projections = outgoing_edges
            .into_iter()
            .filter_map(|parent_edge| {
                let dep = graph.edge_content(&parent_edge);

                let res: Option<ModelProjection> = match dep {
                    Some(QueryDependency::InjectFilter(f)) => Some(f.clone().into()),
                    Some(QueryDependency::InjectData(i)) => Some(i.clone().into()),
                    None => None,
                };

                res
            })
            .collect();

        Some(ModelProjection::union(expected_projections))
    }

    fn query_transform_expression(query: Query, transformers: Vec<QueryTransformer>) -> Expression {
        if transformers.is_empty() {
            Expression::Data(Data::Query(query.clone()))
        } else {
            Expression::Invoke(FnInvocation::TransformQuery(query.clone(), transformers))
        }
    }

    fn derive_reload_query_expression(
        query: &Query,
        parent_query_id: String,
        projection: ModelProjection,
    ) -> Expression {
        // let model =

        // Expression::Invoke(FnInvocation::Query(Box::new(Expression::Data(Data::Query(
        //     reload_query,
        // )))))

        todo!()
    }

    // fn query_connector()

    // fn build_query_expression(
    //     graph: &mut QueryGraph,
    //     node: &NodeRef,
    //     parent_edges: Vec<EdgeRef>,
    // ) -> InterpretationResult<Expression> {
    //     // Child edges are ordered, evaluation order is low to high in the graph, unless other rules override.
    //     let mut direct_children = graph.direct_child_pairs(&node);

    //     // Find the positions of all result returning graph nodes.
    //     let result_positions: Vec<usize> = direct_children
    //         .iter()
    //         .enumerate()
    //         .filter_map(|(ix, (_, child_node))| {
    //             if graph.subgraph_contains_result(&child_node) {
    //                 Some(ix)
    //             } else {
    //                 None
    //             }
    //         })
    //         .collect();

    //     let result_subgraphs: Vec<(EdgeRef, NodeRef)> = result_positions
    //         .into_iter()

    //         .map(|pos| direct_children.remove(pos))
    //         .collect();

    //     // Because we split from right to left, everything remaining in `direct_children`
    //     // doesn't belong into results, and is executed before all result scopes.
    //     let mut expressions: Vec<Expression> = direct_children
    //         .into_iter()
    //         .map(|(_, node)| {
    //             let edges = graph.incoming_edges(&node);
    //             Self::build_expression(graph, &node, edges)
    //         })
    //         .collect::<InterpretationResult<Vec<Expression>>>()?;

    //     // Fold result scopes into one expression.
    //     if result_subgraphs.len() > 0 {
    //         let result_exp = Self::fold_result_scopes(graph, result_subgraphs)?;
    //         expressions.push(result_exp);
    //     }

    //     let is_result = graph.is_result_node(&node);
    //     let node_id = node.id();
    //     let node = graph.pluck_node(&node);
    //     let into_expr = Box::new(|node: Node| {
    //         let query: Query = node.try_into()?;
    //         Ok(Expression::Query { query })
    //     });

    //     let expr = Self::transform_node(graph, parent_edges, node, into_expr)?;

    //     if expressions.is_empty() {
    //         Ok(expr)
    //     } else {
    //         let node_binding_name = node_id.clone();

    //         // Add a final statement to the evaluation if the current node has child nodes and is supposed to be the
    //         // final result, to make sure it propagates upwards.
    //         if is_result {
    //             expressions.push(Expression::Get {
    //                 binding_name: node_binding_name.clone(),
    //             });
    //         }

    //         Ok(Expression::Let {
    //             bindings: vec![Binding {
    //                 name: node_binding_name,
    //                 expr,
    //             }],
    //             expressions,
    //         })
    //     }
    // }

    // fn build_empty_expression(
    //     graph: &mut QueryGraph,
    //     node: &NodeRef,
    //     parent_edges: Vec<EdgeRef>,
    // ) -> InterpretationResult<Expression> {
    //     let child_pairs = graph.direct_child_pairs(node);

    //     let exprs: Vec<Expression> = child_pairs
    //         .into_iter()
    //         .map(|(_, node)| Self::build_expression(graph, &node, graph.incoming_edges(&node)))
    //         .collect::<InterpretationResult<_>>()?;

    //     let into_expr = Box::new(move |_node: Node| Ok(Expression::Sequence { seq: exprs }));
    //     Self::transform_node(graph, parent_edges, Node::Empty, into_expr)
    // }

    // fn build_computation_expression(
    //     graph: &mut QueryGraph,
    //     node: &NodeRef,
    //     parent_edges: Vec<EdgeRef>,
    // ) -> InterpretationResult<Expression> {
    //     let node_id = node.id();
    //     let child_pairs = graph.direct_child_pairs(node);

    //     let exprs: Vec<Expression> = child_pairs
    //         .into_iter()
    //         .map(|(_, node)| Self::build_expression(graph, &node, graph.incoming_edges(&node)))
    //         .collect::<InterpretationResult<_>>()?;

    //     let node = graph.pluck_node(node);
    //     let into_expr = Box::new(move |node: Node| {
    //         Ok(Expression::Func {
    //             func: Box::new(move |_| match node {
    //                 Node::Computation(Computation::Diff(DiffNode { left, right })) => {
    //                     let left_diff: Vec<&RecordProjection> = left.difference(&right).collect();
    //                     let right_diff: Vec<&RecordProjection> = right.difference(&left).collect();

    //                     Ok(Expression::Return {
    //                         result: ExpressionResult::Computation(ComputationResult::Diff(DiffResult {
    //                             left: left_diff.into_iter().map(Clone::clone).collect(),
    //                             right: right_diff.into_iter().map(Clone::clone).collect(),
    //                         })),
    //                     })
    //                 }
    //                 _ => unreachable!(),
    //             }),
    //         })
    //     });

    //     let expr = Self::transform_node(graph, parent_edges, node, into_expr)?;

    //     if exprs.is_empty() {
    //         Ok(expr)
    //     } else {
    //         let node_binding_name = node_id.clone();

    //         Ok(Expression::Let {
    //             bindings: vec![Binding {
    //                 name: node_binding_name,
    //                 expr,
    //             }],
    //             expressions: exprs,
    //         })
    //     }
    // }

    // fn build_flow_expression(
    //     graph: &mut QueryGraph,
    //     node: &NodeRef,
    //     parent_edges: Vec<EdgeRef>,
    // ) -> InterpretationResult<Expression> {
    //     match graph.node_content(node).unwrap() {
    //         Node::Flow(Flow::If(_)) => {
    //             let child_pairs = graph.child_pairs(node);

    //             // Graph validation guarantees this succeeds.
    //             let (mut then_pair, mut else_pair): (Vec<(EdgeRef, NodeRef)>, Vec<(EdgeRef, NodeRef)>) = child_pairs
    //                 .into_iter()
    //                 .partition(|(edge, _)| match graph.edge_content(&edge).unwrap() {
    //                     QueryGraphDependency::Then => true,
    //                     QueryGraphDependency::Else => false,
    //                     _ => unreachable!(),
    //                 });

    //             let then_pair = then_pair.pop().unwrap();
    //             let else_pair = else_pair.pop();

    //             // Build expressions for both arms. They are treated as separate root nodes.
    //             let then_expr = Self::build_expression(graph, &then_pair.1, graph.incoming_edges(&then_pair.1))?;
    //             let else_expr = else_pair
    //                 .into_iter()
    //                 .map(|(_, node)| Self::build_expression(graph, &node, graph.incoming_edges(&node)))
    //                 .collect::<InterpretationResult<Vec<Expression>>>()?;

    //             let node = graph.pluck_node(node);
    //             let into_expr = Box::new(move |node: Node| {
    //                 let flow: Flow = node.try_into()?;
    //                 match flow {
    //                     Flow::If(cond_fn) => Ok(Expression::If {
    //                         func: cond_fn,
    //                         then: vec![then_expr],
    //                         else_: else_expr,
    //                     }),
    //                 }
    //             });

    //             Self::transform_node(graph, parent_edges, node, into_expr)
    //         }
    //         _ => unreachable!(),
    //     }
    // }

    // /// Runs transformer functions (e.g. `ParentIdsFn`) via `Expression::Func` if necessary, or if none present,
    // /// builds an expression directly. `into_expr` does the final expression building based on the node coming in.
    // fn transform_node(
    //     graph: &mut QueryGraph,
    //     parent_edges: Vec<EdgeRef>,
    //     node: Node,
    //     into_expr: Box<dyn FnOnce(Node) -> InterpretationResult<Expression> + Send + Sync + 'static>,
    // ) -> InterpretationResult<Expression> {
    //     if parent_edges.is_empty() {
    //         into_expr(node)
    //     } else {
    //         // Collect all parent ID dependency tuples (transformers).
    //         let parent_id_deps = Self::collect_parent_transformers(graph, parent_edges);

    //         // If there is at least one parent ID dependency we build a func to run the transformer(s),
    //         // else just render a flat query expression.
    //         if parent_id_deps.is_empty() {
    //             into_expr(node)
    //         } else {
    //             Ok(Expression::Func {
    //                 func: Box::new(move |env: Env| {
    //                     // Run transformers in order on the query to retrieve the final, transformed, query.
    //                     let node: InterpretationResult<Node> =
    //                         parent_id_deps
    //                             .into_iter()
    //                             .try_fold(node, |node, (parent_binding_name, dependency)| {
    //                                 let binding = match env.get(&parent_binding_name) {
    //                                     Some(binding) => Ok(binding),
    //                                     None => Err(InterpreterError::EnvVarNotFound(format!(
    //                                         "Expected parent binding '{}' to be present.",
    //                                         parent_binding_name
    //                                     ))),
    //                                 }?;

    //                                 let res = match dependency {
    //                                     QueryGraphDependency::ParentProjection(projection, f) => binding
    //                                         .as_projections(&projection)
    //                                         .and_then(|parent_projections| Ok(f(node, parent_projections)?)),

    //                                     QueryGraphDependency::ParentResult(f) => Ok(f(node, &binding)?),

    //                                     _ => unreachable!(),
    //                                 };

    //                                 Ok(res.map_err(|err| {
    //                                     InterpreterError::InterpretationError(
    //                                         format!("Error for binding '{}'", parent_binding_name),
    //                                         Some(Box::new(err)),
    //                                     )
    //                                 })?)
    //                             });

    //                     into_expr(node?)
    //                 }),
    //             })
    //         }
    //     }
    // }

    // /// Collects all edge dependencies that perform a node transformation based on the parent.
    // fn collect_parent_transformers(
    //     graph: &mut QueryGraph,
    //     parent_edges: Vec<EdgeRef>,
    // ) -> Vec<(String, QueryGraphDependency)> {
    //     parent_edges
    //         .into_iter()
    //         .filter_map(|edge| match graph.pluck_edge(&edge) {
    //             x @ QueryGraphDependency::ParentResult(_) => {
    //                 let parent_binding_name = graph.edge_source(&edge).id();
    //                 Some((parent_binding_name, x))
    //             }
    //             x @ QueryGraphDependency::ParentProjection(_, _) => {
    //                 let parent_binding_name = graph.edge_source(&edge).id();
    //                 Some((parent_binding_name, x))
    //             }
    //             _ => None,
    //         })
    //         .collect()
    // }

    // fn fold_result_scopes(
    //     graph: &mut QueryGraph,
    //     result_subgraphs: Vec<(EdgeRef, NodeRef)>,
    // ) -> InterpretationResult<Expression> {
    //     let bindings: Vec<Binding> = result_subgraphs
    //         .into_iter()
    //         .map(|(_, node)| {
    //             let name = node.id();
    //             let edges = graph.incoming_edges(&node);
    //             let expr = Self::build_expression(graph, &node, edges)?;

    //             Ok(Binding { name, expr })
    //         })
    //         .collect::<InterpretationResult<Vec<Binding>>>()?;

    //     let result_binding_names = bindings.iter().map(|b| b.name.clone()).collect();

    //     Ok(Expression::Let {
    //         bindings,
    //         expressions: vec![Expression::GetFirstNonEmpty {
    //             binding_names: result_binding_names,
    //         }],
    //     })
    // }
}