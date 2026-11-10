use super::*;
use crate::ScriptedSql;

fn refused() -> SqlError {
    SqlError::Unavailable {
        reason: "UNIQUE constraint failed".to_owned(),
    }
}

/// What a database that keeps every promise answers each transaction with.
fn kept() -> [Result<Vec<Changed>, SqlError>; 5] {
    [
        Ok(vec![
            changed(0),
            changed(0),
            changed(2),
            changed(0),
            rows(vec![
                vec![
                    Value::Integer(1),
                    Value::Integer(9_007_199_254_740_991),
                    Value::Text("héraldé ✓".to_owned()),
                ],
                vec![
                    Value::Integer(2),
                    Value::Integer(-9_007_199_254_740_991),
                    Value::Null,
                ],
            ]),
        ]),
        Err(refused()),
        Ok(vec![rows(vec![vec![Value::Integer(2)]])]),
        Ok(vec![
            changed(1),
            changed(0),
            changed(1),
            rows(vec![vec![
                Value::Integer(2),
                Value::Text("updated".to_owned()),
            ]]),
            rows(Vec::new()),
        ]),
        Ok(vec![changed(0)]),
    ]
}

fn scripted(answers: [Result<Vec<Changed>, SqlError>; 5]) -> ScriptedSql {
    answers
        .into_iter()
        .fold(ScriptedSql::default(), ScriptedSql::answering)
}

fn run(answers: [Result<Vec<Changed>, SqlError>; 5]) -> Result<(), Failure> {
    pollster::block_on(sql_conformance(&scripted(answers), Parameters::Question))
}

#[test]
fn the_scripted_database_passes_sending_each_statement_as_its_database_writes_it() {
    let sql = scripted(kept());
    assert_eq!(
        pollster::block_on(sql_conformance(&sql, Parameters::Dollar)),
        Ok(())
    );
    let sent = sql.sent();
    assert_eq!(sent.len(), 5);
    assert_eq!(
        sent[0][2],
        Statement::change(
            "INSERT INTO herald_conformance (k, n, t) VALUES (1, $1, $2), (2, $3, $4)",
            vec![
                Value::Integer(9_007_199_254_740_991),
                Value::Text("héraldé ✓".to_owned()),
                Value::Integer(-9_007_199_254_740_991),
                Value::Null,
            ]
        )
    );
    assert_eq!(
        sent[3][0],
        Statement::change(
            "UPDATE herald_conformance SET t = $1 WHERE n < $2",
            vec![Value::Text("updated".to_owned()), Value::Integer(0)]
        )
    );
    assert_eq!(
        sent[4],
        [Statement::change("DROP TABLE herald_conformance", vec![])]
    );
    assert_eq!(run(kept()), Ok(()));
}

#[test]
fn a_database_that_breaks_its_promises_is_refused() {
    let mut stale = kept();
    stale[0] = Ok(vec![
        changed(0),
        changed(0),
        changed(2),
        changed(2),
        rows(Vec::new()),
    ]);
    let mut misread = kept();
    if let Ok(answers) = &mut misread[0] {
        answers[4].rows[0][1] = Value::Text("9007199254740991".to_owned());
    }
    let mut unanswered = kept();
    if let Ok(answers) = &mut unanswered[0] {
        answers.push(changed(0));
    }
    let mut taken = kept();
    taken[1] = Ok(vec![changed(1), changed(1)]);
    let mut kept_half = kept();
    kept_half[2] = Ok(vec![rows(vec![vec![Value::Integer(3)]])]);
    let mut uncounted = kept();
    if let Ok(answers) = &mut uncounted[3] {
        answers[1] = changed(1);
    }
    let mut unready = kept();
    unready[0] = Err(refused());
    let cases = [
        (
            stale,
            "a statement did not answer how many rows it changed, or answered for one that changed none",
        ),
        (
            misread,
            "a row was not read back with every value as it was bound",
        ),
        (
            unanswered,
            "a transaction did not answer each of its statements",
        ),
        (
            taken,
            "a transaction holding a statement the database refuses was not refused",
        ),
        (
            kept_half,
            "a transaction that was refused kept a statement it ran before the refusal",
        ),
        (
            uncounted,
            "an update or delete did not answer how many rows it changed",
        ),
        (
            unready,
            "the database failed: the database refused the statements: UNIQUE constraint failed",
        ),
    ];
    for (answers, failure) in cases {
        assert_eq!(run(answers), Err(Failure(failure.to_owned())), "{failure}");
    }
    let mut undropped = kept();
    undropped[4] = Err(refused());
    assert_eq!(
        run(undropped),
        Err(Failure(
            "the database failed: the database refused the statements: UNIQUE constraint failed"
                .to_owned()
        ))
    );
}
