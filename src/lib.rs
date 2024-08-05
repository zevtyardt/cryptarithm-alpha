use cached::proc_macro::cached;
use crossbeam_channel::Sender;
use itertools::Itertools;
use std::collections::{BTreeMap, BTreeSet};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum Token {
    Word(Vec<char>),
    Operator(char),
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Equation {
    // Reverse Polish notation (RPN)
    rpn_token: Vec<Token>,
    result: Vec<char>,
    pub mapping: BTreeMap<char, u8>,

    unique_chars: BTreeSet<char>,
    special_chars: BTreeSet<char>,
}

fn apply_operation(equation: &Equation) -> anyhow::Result<bool> {
    let mut stack = vec![];
    for token in equation.rpn_token.iter() {
        match token {
            Token::Word(bytes) => {
                let num = bytes
                    .iter()
                    .map(|c| *equation.mapping.get(c).unwrap_or(&1))
                    .join("")
                    .parse::<u32>()
                    .unwrap();
                stack.push(num);
            }
            Token::Operator(op) => {
                if stack.len() < 2 {
                    anyhow::bail!("Invalid RPN expresion");
                }
                let b = stack.pop().unwrap();
                let a = stack.pop().unwrap();
                let res = match op {
                    '+' => a + b,
                    '-' => a - b,
                    '*' => a * b,
                    '/' => a / b,
                    _ => anyhow::bail!("Invalid operator: {}", op),
                };
                stack.push(res);
            }
        }
    }

    if let Some(output) = stack.pop() {
        let result = equation
            .result
            .iter()
            .map(|c| *equation.mapping.get(c).unwrap_or(&1))
            .join("")
            .parse::<u32>()?;

        return Ok(output == result);
    }
    anyhow::bail!("Invalid RPN expresion")
}

fn backtrack(
    equation: &mut Equation,
    digits: &[u8],
    rx: &Sender<BTreeMap<char, u8>>,
) -> anyhow::Result<bool> {
    if equation.unique_chars.is_empty() {
        return apply_operation(equation);
    }

    let ch = equation
        .unique_chars
        .pop_first()
        .expect("unique_chars is empty");
    for &digit in digits {
        if digit == 0 && equation.special_chars.contains(&ch) {
            continue;
        }
        if !equation.mapping.values().any(|&v| v == digit) {
            equation.mapping.insert(ch, digit);
            if backtrack(equation, digits, rx)? {
                rx.send(equation.mapping.clone()).unwrap_or(());
                return Ok(true);
            }
            equation.mapping.remove(&ch);
        }
    }
    equation.unique_chars.insert(ch);
    Ok(false)
}

pub fn parse_input(expresion: &str) -> anyhow::Result<Equation> {
    fn is_operator(c: char) -> bool {
        c == '+' || c == '-' || c == '*' || c == '/'
    }

    fn precedence(op: char) -> i32 {
        match op {
            '+' | '-' => 1,
            '*' | '/' => 2,
            _ => 0,
        }
    }

    let mut expresion = expresion.to_string();
    for op in ['+', '-', '*', '/'] {
        expresion = expresion.replace(op, &format!(" {} ", op));
    }

    let mut equation = Equation {
        rpn_token: vec![],
        result: vec![],
        mapping: BTreeMap::new(),

        unique_chars: BTreeSet::new(),
        special_chars: BTreeSet::new(),
    };

    let mut buffer = vec![];
    let mut operators = vec![];
    for ch in expresion.chars() {
        if ch.is_alphabetic() {
            equation.unique_chars.insert(ch);
            buffer.push(ch);
            continue;
        } else if !is_operator(ch) && ch != ' ' && ch != '=' {
            anyhow::bail!("Invalid token {}", ch);
        }

        if !buffer.is_empty() {
            equation.special_chars.insert(buffer[0]);
            equation.rpn_token.push(Token::Word(buffer.split_off(0)));
        } else if is_operator(ch) {
            while let Some(&top) = operators.last() {
                if precedence(top) >= precedence(ch) {
                    equation
                        .rpn_token
                        .push(Token::Operator(operators.pop().unwrap()))
                } else {
                    break;
                }
            }
            operators.push(ch)
        }
    }
    if !buffer.is_empty() {
        equation.special_chars.insert(buffer[0]);
        equation.result = buffer
    }

    while let Some(op) = operators.pop() {
        equation.rpn_token.push(Token::Operator(op));
    }
    apply_operation(&equation)?;
    Ok(equation)
}

#[cached(size = 1024, time = 120)]
pub fn solve(equation: Equation) -> BTreeMap<char, u8> {
    let (rx, tx) = crossbeam_channel::bounded(1);

    let mut handlers = vec![];
    for digits in [(0..=9).rev().collect_vec(), (0..=9).collect_vec()] {
        let mut equation = equation.clone();
        let rx = rx.clone();
        handlers.push(thread::spawn(move || {
            backtrack(&mut equation, &digits, &rx).unwrap_or_default();
        }));
    }

    thread::spawn(move || {
        for task in handlers {
            task.join().unwrap();
        }
        rx.send(BTreeMap::new()).unwrap_or_default();
    });
    tx.recv_timeout(Duration::from_secs(7)).unwrap_or_default()
}
