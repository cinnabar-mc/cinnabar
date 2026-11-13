#!/usr/bin/env bash
# Sourced by the APK packager after its pinned values and product identity are loaded.
# @APPLICATION_ID@ is the Java package; an overridden installed ID is applied when linking.
# @PRODUCT_NAME@ is the label the installed app shows.

# sed_value <text>: escape what a sed replacement delimited by | would otherwise interpret.
sed_value() {
    printf '%s' "$1" | sed -e 's/[\\&|]/\\&/g'
}

# render <source> <destination>: fill packaging values and reject unresolved template tokens.
# The build commit and release channel come from the variables the client itself is built with.
render() {
    sed -e "s|@APPLICATION_ID@|$(pin application_id)|g" -e "s|@CLIENT_LIBRARY@|$client_library|g" \
        -e "s|@PRODUCT_NAME@|${app_label:-$product}|g" -e "s|@VERSION_CODE@|$version_code|g" \
        -e "s|@VERSION_NAME@|$version|g" -e "s|@PROFILEABLE@|${profileable:-false}|g" \
        -e "s|@ADAPTER_RECORD@|$(pin adapter_record)|g" \
        -e "s|@BUILD_COMMIT@|$(sed_value "${CINNABAR_BUILD_COMMIT:-unknown}")|g" \
        -e "s|@RELEASE_CHANNEL@|$(sed_value "${CINNABAR_RELEASE_CHANNEL:-unpublished}")|g" "$1" >"$2"
    if grep -q '@[A-Z_]*@' "$2"; then
        echo "unfilled template token in $2" >&2
        exit 1
    fi
}
