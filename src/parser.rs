use crate::ast::{AstNode, Expression, Statement};
use crate::error::ReportableError;
use crate::scanner::Scanner;
use crate::token::{Kind, Span, Token};
use crate::value::Value;

#[derive(Debug)]
pub struct ParsingError {
    message: String,
    span: Span,
}

impl ReportableError for ParsingError {
    fn span(&self) -> Span {
        self.span
    }
    fn message(&self) -> String {
        format!("Parsing Error - {}", self.message)
    }
}

#[derive(Debug)]
pub struct Parser {
    scanner: Scanner,
    current: Token,
    next: Token,
}

impl Parser {
    pub fn new(source: &str) -> Self {
        let mut scanner = Scanner::new(&source);
        let current = scanner.next().unwrap();
        let next = scanner.next().unwrap();
        Parser {
            scanner,
            current,
            next,
        }
    }

    /// Parse the source into a program - a list of declaratation `AstNode`s
    pub fn parse_program(&mut self) -> Result<Vec<AstNode>, Vec<ParsingError>> {
        let mut program = vec![];
        let mut errors = vec![];

        while self.current.kind != Kind::Eof {
            match self.declaration() {
                Ok(decl) => program.push(decl),
                Err(err) => {
                    self.synchronize();
                    errors.push(err);
                }
            }
        }

        if errors.is_empty() {
            Ok(program)
        } else {
            Err(errors.drain(0..).collect())
        }
    }

    fn declaration(&mut self) -> Result<AstNode, ParsingError> {
        match self.current.kind {
            Kind::Var => self.var_declaration(),
            Kind::Fun => {
                self.advance();
                self.function()
            }
            _ => self.statement(),
        }
    }

    fn id_token(&mut self) -> Result<(String, Span), ParsingError> {
        let Token { kind, span } = self.advance();
        if let Kind::IdentifierLiteral(id) = kind {
            Ok((id, span))
        } else {
            Err(ParsingError {
                message: "Expected identifier.".to_string(),
                span,
            })
        }
    }

    fn var_declaration(&mut self) -> Result<AstNode, ParsingError> {
        let keyword = self.advance();
        let (name, _) = self.id_token()?;

        let initializer = if self.current.kind == Kind::Equal {
            self.advance();
            let initializer = self.expression()?;
            Some(Box::new(initializer))
        } else {
            None
        };

        let semi = self.eat(Kind::Semicolon, "Expected ';' after declaration.")?;
        let span = Span::merge(vec![&keyword.span, &semi.span]);
        Ok(AstNode::new_statement(
            Statement::Declaration { name, initializer },
            span,
        ))
    }

    fn parameter_list(&mut self) -> Result<Vec<Token>, ParsingError> {
        let mut parameters = vec![];
        parameters.push(self.advance());
        while self.current.kind == Kind::Comma {
            self.advance();
            let param_name = self.advance();
            if let Kind::IdentifierLiteral(_) = param_name.kind {
                parameters.push(param_name);
            } else {
                return Err(ParsingError {
                    message: "Expected parameter name.".to_string(),
                    span: param_name.span,
                });
            }
        }

        Ok(parameters)
    }

    fn function(&mut self) -> Result<AstNode, ParsingError> {
        let (name, name_span) = self.id_token()?;
        self.eat(Kind::LeftParen, "Expected '(' after function name")?;

        let parameters = match self.current.kind {
            Kind::RightParen => vec![],
            Kind::IdentifierLiteral(_) => self.parameter_list()?,
            _ => {
                return Err(ParsingError {
                    message: "Expected parameter list or ')'.".to_string(),
                    span: self.current.span,
                })
            }
        };

        self.eat(Kind::RightParen, "Expected ')' after formal parameter list")?;
        let body = self.block_statement()?;
        let span = Span::merge(vec![&name_span, &body.span]);

        Ok(AstNode::new_statement(
            Statement::FunDeclaration {
                name,
                parameters,
                body: Box::new(body),
            },
            span,
        ))
    }

    fn statement(&mut self) -> Result<AstNode, ParsingError> {
        match self.current.kind {
            Kind::Print => self.print_statement(),
            Kind::LeftBrace => self.block_statement(),
            Kind::If => self.if_statement(),
            Kind::While => self.while_statement(),
            Kind::For => self.for_statement(),
            Kind::Return => self.return_statement(),
            _ => self.expression_statement(),
        }
    }

    fn expression_statement(&mut self) -> Result<AstNode, ParsingError> {
        let expression = self.expression()?;
        let semi = self.eat(Kind::Semicolon, "Expected ';' after expression")?;
        let new_span = Span::merge(vec![&expression.span, &semi.span]);
        Ok(AstNode::new_statement(
            Statement::Expression {
                expression: Box::new(expression),
            },
            new_span,
        ))
    }

    fn return_statement(&mut self) -> Result<AstNode, ParsingError> {
        let keyword = self.advance();

        let (value, span) = match self.current.kind {
            Kind::Semicolon => (None, keyword.span),
            _ => {
                let expr = self.expression()?;
                let span = Span::merge(vec![&keyword.span, &expr.span]);
                (Some(Box::new(expr)), span)
            }
        };

        self.eat(Kind::Semicolon, "Expected ';' after return statement.")?;
        Ok(AstNode::new_statement(Statement::Return { value }, span))
    }

    fn for_statement(&mut self) -> Result<AstNode, ParsingError> {
        let keyword = self.advance();
        self.eat(Kind::LeftParen, "Expected '(' after 'for.'")?;

        let initializer = match self.current.kind {
            Kind::Var => Some(Box::new(self.var_declaration()?)),
            Kind::Semicolon => {
                self.advance();
                None
            }
            _ => Some(Box::new(self.expression_statement()?)),
        };

        let condition = match self.current.kind {
            Kind::Semicolon => None,
            _ => Some(Box::new(self.expression()?)),
        };

        self.eat(Kind::Semicolon, "Expected ';' after for condition.")?;

        let update = match self.current.kind {
            Kind::RightParen => None,
            _ => Some(Box::new(self.expression()?)),
        };

        self.eat(Kind::RightParen, "Expected ')' before for block.")?;

        let block = self.statement()?;
        let span = Span::merge(vec![&keyword.span, &block.span]);

        Ok(AstNode::new_statement(
            Statement::For {
                initializer,
                condition,
                update,
                block: Box::new(block),
            },
            span,
        ))
    }

    fn while_statement(&mut self) -> Result<AstNode, ParsingError> {
        let keyword = self.advance();
        self.eat(Kind::LeftParen, "Expected '(' after 'while.'")?;

        let condition = self.expression()?;
        self.eat(Kind::RightParen, "Expected ')' after while condition.")?;

        let block = self.statement()?;
        let span = Span::merge(vec![&keyword.span, &block.span]);

        Ok(AstNode::new_statement(
            Statement::While {
                condition: Box::new(condition),
                block: Box::new(block),
            },
            span,
        ))
    }

    fn if_statement(&mut self) -> Result<AstNode, ParsingError> {
        let keyword = self.advance();
        self.eat(Kind::LeftParen, "Expected '(' after 'if.'")?;
        let condition = self.expression()?;

        self.eat(Kind::RightParen, "Expected ')' after if condition.")?;

        let if_block = self.statement()?;
        let mut span = Span::merge(vec![&keyword.span, &if_block.span]);

        let else_block = if let Kind::Else = self.current.kind {
            self.advance();
            let stmt = self.statement()?;
            span = Span::merge(vec![&span, &stmt.span]);
            Some(Box::new(stmt))
        } else {
            None
        };

        Ok(AstNode::new_statement(
            Statement::If {
                condition: Box::new(condition),
                if_block: Box::new(if_block),
                else_block,
            },
            span,
        ))
    }

    fn block_statement(&mut self) -> Result<AstNode, ParsingError> {
        let lbrace = self.advance();

        let mut declarations = vec![];
        loop {
            match self.current.kind {
                Kind::RightBrace | Kind::Eof => break,
                _ => declarations.push(self.declaration()?),
            }
        }

        let rbrace = self.eat(Kind::RightBrace, "Expected '}' after block statement")?;
        let new_span = Span::merge(vec![&lbrace.span, &rbrace.span]);
        Ok(AstNode::new_statement(
            Statement::Block {
                declarations,
                rbrace,
            },
            new_span,
        ))
    }

    fn print_statement(&mut self) -> Result<AstNode, ParsingError> {
        let keyword = self.advance();
        let expression = self.expression()?;
        let semi = self.eat(Kind::Semicolon, "Expected ';' after print statement")?;
        let new_span = Span::merge(vec![&keyword.span, &expression.span, &semi.span]);
        Ok(AstNode::new_statement(
            Statement::Print {
                expression: Box::new(expression),
            },
            new_span,
        ))
    }

    fn expression(&mut self) -> Result<AstNode, ParsingError> {
        self.assignment()
    }

    fn assignment(&mut self) -> Result<AstNode, ParsingError> {
        let node = self.logic_or()?;

        if self.current.kind == Kind::Equal {
            let operator = self.advance();
            let rvalue = self.assignment()?;
            let new_span = Span::merge(vec![&node.span, &operator.span, &rvalue.span]);

            Ok(AstNode::new_expression(
                Expression::Assignment {
                    lvalue: Box::new(node),
                    operator,
                    rvalue: Box::new(rvalue),
                },
                new_span,
            ))
        } else {
            Ok(node)
        }
    }

    fn logic_or(&mut self) -> Result<AstNode, ParsingError> {
        self.logic_and()
    }

    fn logic_and(&mut self) -> Result<AstNode, ParsingError> {
        self.equality()
    }

    fn equality(&mut self) -> Result<AstNode, ParsingError> {
        let mut node = self.comparison()?;
        while let Kind::EqualEqual | Kind::BangEqual = self.current.kind {
            let operator = self.advance();
            let right = self.comparison()?;
            let new_span = Span::merge(vec![&node.span, &operator.span, &right.span]);

            node = AstNode::new_expression(
                Expression::Binary {
                    left: Box::new(node),
                    operator,
                    right: Box::new(right),
                },
                new_span,
            );
        }
        Ok(node)
    }

    fn comparison(&mut self) -> Result<AstNode, ParsingError> {
        let mut node = self.addition()?;
        while let Kind::Less | Kind::LessEqual | Kind::Greater | Kind::GreaterEqual =
            self.current.kind
        {
            let operator = self.advance();
            let right = self.addition()?;
            let new_span = Span::merge(vec![&node.span, &operator.span, &right.span]);

            node = AstNode::new_expression(
                Expression::Binary {
                    left: Box::new(node),
                    operator,
                    right: Box::new(right),
                },
                new_span,
            );
        }
        Ok(node)
    }

    fn addition(&mut self) -> Result<AstNode, ParsingError> {
        let mut node = self.multiplication()?;

        while self.current.kind == Kind::Plus || self.current.kind == Kind::Minus {
            let operator = self.advance();
            let right = self.multiplication()?;
            let new_span = Span::merge(vec![&node.span, &operator.span, &right.span]);

            node = AstNode::new_expression(
                Expression::Binary {
                    left: Box::new(node),
                    operator,
                    right: Box::new(right),
                },
                new_span,
            );
        }

        Ok(node)
    }

    fn multiplication(&mut self) -> Result<AstNode, ParsingError> {
        let mut node = self.unary()?;

        while self.current.kind == Kind::Star || self.current.kind == Kind::Slash {
            let operator = self.advance();
            let right = self.unary()?;
            let new_span = Span::merge(vec![&node.span, &operator.span, &right.span]);

            node = AstNode::new_expression(
                Expression::Binary {
                    left: Box::new(node),
                    operator,
                    right: Box::new(right),
                },
                new_span,
            );
        }

        Ok(node)
    }

    fn unary(&mut self) -> Result<AstNode, ParsingError> {
        match self.current.kind {
            Kind::Minus | Kind::Bang => {
                let operator = self.advance();
                let expression = self.unary()?;
                let new_span = Span::new(expression.span.start - 1, expression.span.end);

                Ok(AstNode::new_expression(
                    Expression::Unary {
                        operator,
                        expression: Box::new(expression),
                    },
                    new_span,
                ))
            }
            _ => self.call(),
        }
    }

    fn argument_list(&mut self) -> Result<Vec<AstNode>, ParsingError> {
        let mut args = vec![];
        args.push(self.expression()?);
        while self.current.kind == Kind::Comma {
            self.advance();
            args.push(self.expression()?);
        }

        Ok(args)
    }

    fn call(&mut self) -> Result<AstNode, ParsingError> {
        let primary = self.primary()?;

        if self.current.kind == Kind::LeftParen {
            self.advance();

            let arguments = match self.current.kind {
                Kind::RightParen => vec![],
                _ => self.argument_list()?,
            };

            let rparen = self.eat(Kind::RightParen, "Expected ')' after argument list.")?;

            let new_span = Span::merge(vec![&primary.span, &rparen.span]);

            Ok(AstNode::new_expression(
                Expression::Call {
                    target: Box::new(primary),
                    arguments,
                },
                new_span,
            ))
        } else {
            Ok(primary)
        }
    }

    fn primary(&mut self) -> Result<AstNode, ParsingError> {
        match self.current.clone().kind {
            Kind::LeftParen => {
                let lparen = self.advance();
                let expression = self.expression()?;
                let rparen = self.eat(Kind::RightParen, "Expected ')' after expression.")?;
                let new_span = Span::merge(vec![&lparen.span, &expression.span, &rparen.span]);
                Ok(AstNode::new_ast_node(expression, new_span))
            }
            Kind::IdentifierLiteral(name) => Ok(AstNode::new_expression(
                Expression::Variable { name },
                self.advance().span,
            )),
            Kind::NumberLiteral(_) => self.number(),
            Kind::StringLiteral(_) => self.string(),
            Kind::True => Ok(AstNode::new_expression(
                Expression::Constant {
                    value: Value::Bool(true),
                },
                self.advance().span,
            )),
            Kind::False => Ok(AstNode::new_expression(
                Expression::Constant {
                    value: Value::Bool(false),
                },
                self.advance().span,
            )),
            Kind::Nil => {
                let literal = self.advance();
                let span = literal.span;
                Ok(AstNode::new_expression(
                    Expression::Constant { value: Value::Nil },
                    span,
                ))
            }
            _ => Err(ParsingError {
                span: self.current.span,
                message: "Expected primary expression.".to_string(),
            }),
        }
    }

    fn number(&mut self) -> Result<AstNode, ParsingError> {
        let Token { kind, span } = self.advance();

        if let Kind::NumberLiteral(n) = kind {
            Ok(AstNode::new_expression(
                Expression::Constant {
                    value: Value::from(n),
                },
                span,
            ))
        } else {
            Err(ParsingError {
                span,
                message: "Expected a NumberLiteral.".to_string(),
            })
        }
    }

    fn string(&mut self) -> Result<AstNode, ParsingError> {
        let Token { kind, span } = self.advance();
        if let Kind::StringLiteral(s) = kind {
            Ok(AstNode::new_expression(
                Expression::Constant {
                    value: Value::from(s),
                },
                span,
            ))
        } else {
            Err(ParsingError {
                span,
                message: "Expected a StringLiteral.".to_string(),
            })
        }
    }

    fn advance(&mut self) -> Token {
        let previous = self.current.clone();
        self.current = self.next.clone();
        self.next = self.scanner.next().unwrap();
        previous
    }
    fn eat(&mut self, kind: Kind, message: &str) -> Result<Token, ParsingError> {
        if self.current.kind == kind {
            Ok(self.advance())
        } else {
            Err(ParsingError {
                message: message.to_string(),
                span: self.current.span,
            })
        }
    }

    /// Consume tokens until current is '{', '}', or the token after a ';'
    fn synchronize(&mut self) {
        loop {
            match self.current.kind {
                Kind::Semicolon | Kind::Eof => {
                    self.advance();
                    break;
                }
                Kind::LeftBrace | Kind::RightBrace => {
                    break;
                }
                _ => {
                    self.advance();
                }
            }
        }
    }
}
