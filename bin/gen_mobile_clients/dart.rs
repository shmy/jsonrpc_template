use crate::emit::{DocStyle, SourceBuf, banner, doc_comment, each_method, each_model, w};
use crate::model::{Field, Ir, dart_ident};

fn is_primitive(ty: &str) -> bool {
    matches!(
        ty,
        "String"
            | "int"
            | "double"
            | "bool"
            | "dynamic"
            | "Null"
            | "Map<String, dynamic>"
            | "List<dynamic>"
    )
}

fn strip_nullable(ty: &str) -> (&str, bool) {
    ty.strip_suffix('?')
        .map_or((ty, false), |inner| (inner, true))
}

fn list_elem(ty: &str) -> Option<&str> {
    ty.strip_prefix("List<")?.strip_suffix('>')
}

fn from_json_expr(dart_type: &str, access: &str) -> String {
    let (inner, optional) = strip_nullable(dart_type);
    if let Some(elem) = list_elem(inner) {
        let mapper = if is_primitive(elem) {
            format!("(e) => e as {elem}")
        } else {
            format!("(e) => {elem}.fromJson(e as Map<String, dynamic>)")
        };
        let mapped = format!("({access} as List<dynamic>).map({mapper}).toList()");
        if optional {
            format!("{access} == null ? null : {mapped}")
        } else {
            mapped
        }
    } else if is_primitive(inner) {
        if optional {
            format!("{access} as {inner}?")
        } else {
            format!("{access} as {inner}")
        }
    } else if optional {
        format!("{access} == null ? null : {inner}.fromJson({access} as Map<String, dynamic>)")
    } else {
        format!("{inner}.fromJson({access} as Map<String, dynamic>)")
    }
}

fn to_json_expr(field: &Field) -> String {
    let ident = dart_ident(&field.name);
    let (inner, _) = strip_nullable(&field.dart_type);
    if let Some(elem) = list_elem(inner) {
        if is_primitive(elem) {
            ident
        } else if field.optional {
            format!("{ident}?.map((e) => e.toJson()).toList()")
        } else {
            format!("{ident}.map((e) => e.toJson()).toList()")
        }
    } else if is_primitive(inner) {
        ident
    } else if field.optional {
        format!("{ident}?.toJson()")
    } else {
        format!("{ident}.toJson()")
    }
}

pub fn generate(ir: &Ir) -> String {
    let mut o = SourceBuf::default();

    banner(&mut o, ir);
    w!(
        o,
        "// ignore_for_file: non_constant_identifier_names, unintended_html_in_doc_comment"
    );
    o.blank();
    w!(o, "import 'dart:convert';");
    w!(o, "import 'dart:io';");
    o.blank();

    each_model(&mut o, &ir.models, DocStyle::TripleSlash, |o, model| {
        w!(o, "class {} {{", model.name);
        for f in &model.fields {
            doc_comment(o, &f.description, DocStyle::TripleSlash, "  ");
            w!(o, "  final {} {};", f.dart_type, dart_ident(&f.name));
        }
        o.blank();
        let ctor_args = model
            .fields
            .iter()
            .map(|f| {
                let id = dart_ident(&f.name);
                if f.optional {
                    format!("    this.{id},")
                } else {
                    format!("    required this.{id},")
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        w!(o, "  {}({{", model.name);
        o.line(&ctor_args);
        w!(o, "  }});");
        o.blank();
        w!(
            o,
            "  factory {}.fromJson(Map<String, dynamic> json) => {}(",
            model.name,
            model.name
        );
        for f in &model.fields {
            let id = dart_ident(&f.name);
            let expr = from_json_expr(&f.dart_type, &format!("json['{}']", f.name));
            w!(o, "        {id}: {expr},");
        }
        w!(o, "      );");
        o.blank();
        w!(o, "  Map<String, dynamic> toJson() => <String, dynamic>{{");
        for f in &model.fields {
            w!(o, "        '{}': {},", f.name, to_json_expr(f));
        }
        w!(o, "      }};");
        w!(o, "}}");
    });

    o.line(include_str!("templates/dart/runtime.inc").trim_end());
    o.blank();

    w!(o, "class JsonRpcClient {{");
    w!(o, "  JsonRpcClient({{");
    w!(o, "    this.endpoint = '{}',", ir.server);
    w!(o, "    this.accessToken,");
    w!(o, "    this.acceptLanguage,");
    w!(o, "    HttpClient? httpClient,");
    w!(o, "  }}) : _http = httpClient ?? HttpClient();");
    o.blank();
    w!(o, "  String endpoint;");
    w!(o, "  String? accessToken;");
    w!(o, "  String? acceptLanguage;");
    w!(o, "  final HttpClient _http;");
    o.blank();
    o.line(include_str!("templates/dart/client_core.inc").trim_end());
    o.blank();

    each_method(&mut o, &ir.methods, DocStyle::TripleSlash, "  ", |o, m| {
        let auth = m.auth_lit();
        let ret = m
            .result_type
            .as_ref()
            .map(|t| format!("Future<{t}>"))
            .unwrap_or_else(|| "Future<void>".into());
        let sig = m
            .params_type
            .as_ref()
            .map(|pt| format!("{pt} params"))
            .unwrap_or_default();
        let params_expr = if m.params_type.is_some() {
            "params.toJson()"
        } else {
            "null"
        };

        w!(o, "  {ret} {}({sig}) async {{", m.fn_name);
        match &m.result_type {
            Some(rt) => {
                w!(
                    o,
                    "    final result = await _post(method: '{}', params: {params_expr}, requiresAuth: {auth});",
                    m.name
                );
                w!(
                    o,
                    "    return {rt}.fromJson(result as Map<String, dynamic>);"
                );
            }
            None => {
                w!(
                    o,
                    "    await _post(method: '{}', params: {params_expr}, requiresAuth: {auth});",
                    m.name
                );
            }
        }
        w!(o, "  }}");
    });

    w!(o, "  void close() => _http.close(force: true);");
    w!(o, "}}");
    o.blank();
    o.into_string()
}
