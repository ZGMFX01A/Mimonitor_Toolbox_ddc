/// 构造通过 TvService 调用的 app_process 命令行
/// 利用 eval 和 ${IFS} 绕过 Android service call 对参数空白切分的限制
pub fn build_tvservice_app_process_command(jar_path: &str, args: &[&str]) -> String {
    let mut encoded_args = String::new();
    for part in args {
        encoded_args.push_str(&format!("\\${{IFS}}{}", part));
    }

    format!(
        "service call TvService 3 s16 \"sh -c eval\\${{IFS}}CLASSPATH={}\\${{IFS}}/system/bin/app_process\\${{IFS}}/data/data/mitv.service/cache{}\"",
        jar_path, encoded_args
    )
}

/// 构造通过 TvService 调用的通用 shell 命令
pub fn build_tvservice_shell_command(parts: &[&str]) -> String {
    let mut encoded = String::new();
    for part in parts {
        encoded.push_str(&format!("\\${{IFS}}{}", part));
    }
    format!("service call TvService 3 s16 \"sh -c eval{}\"", encoded)
}
