#!/usr/bin/env nu
def main [file: string, name: string] {
    open $file 
    | lines 
    | str replace '= [\w0-9\-]+' '' --all 
    | str replace 'const' '' --all 
    | str replace 'char\*' 'String' --all 
    | str replace 'float' 'f32' --all  
    | str replace 'string' 'String' --all 
    | parse -r '(\w+)\(([\w\-,= ]*?)\)'
    | rename Function Params
    | upsert Params {|p| $p.Params | str trim}
    | upsert Params {
        |i| $i.Params | str trim | split column ',' | transpose | get column1 | str replace '^\s*$' '() x' | str trim | split column ' ' | rename Type Name | str replace 'int' 'i32' --all Type | upsert Name {|n| $n.Name | str snake-case}
        } 
    | each { |i| 
        echo '' 
        echo $"//($i.Function)"
        if ($i.Params | get Type | str collect) == '()' {
            echo $"methods.add_method_mut\(\"($i.Function)\", |_, ($name), _: \(\)| {todo!\(\); Ok\(0\)}\);"
        } else {

            echo $"tealr::mlu::create_named_parameters!\(($i.Function)Params with "
            $i.Params | each { |p| if $p.Type == '()' {echo ''} else {echo $"  ($p.Name) : ($p.Type),"}; echo ''} | str collect
            echo ");"
            echo $"methods.add_method_mut\(\"($i.Function)\", |_, ($name), p: ($i.Function)Params| {todo!\(\); Ok\(0\)}\);"
            }
        echo '' 
    }  | str collect
}