#include <curl/curl.h>
#include <stdio.h>
#include <string.h>
#include <stdlib.h>




static size_t wcb(void *p, size_t s, size_t n, void *d) {
    (void)p; (void)d;
    return s * n;}
extern "C" {
    void turbo_send_message_with_reply(const char* t, const char* c, const char* m, const char* p, int r) {
        CURL *crl = curl_easy_init();
        if(!crl) { return; }
        
        char url[512];
        snprintf(url, sizeof(url), "https://api.telegram.org/bot%s/sendMessage", t);
        
        char *esc = curl_easy_escape(crl, m, 0);
        if(!esc) { curl_easy_cleanup(crl); return; }
        
        char data[4096];
        if(r > 0) { snprintf(data, sizeof(data), "chat_id=%s&text=%s&reply_to_message_id=%d", c, esc, r); }
        else { snprintf(data, sizeof(data), "chat_id=%s&text=%s", c, esc); }
        
        curl_free(esc);
        
        curl_easy_setopt(crl, CURLOPT_URL, url);
        curl_easy_setopt(crl, CURLOPT_POSTFIELDS, data);
        curl_easy_setopt(crl, CURLOPT_WRITEFUNCTION, wcb);
        curl_easy_setopt(crl, CURLOPT_TIMEOUT, 10L);
        
        if(p && p[0] != '\0') {
            curl_easy_setopt(crl, CURLOPT_PROXY, p);
            curl_easy_setopt(crl, CURLOPT_HTTPPROXYTUNNEL, 1L);
        }
    
        curl_easy_perform(crl);
        curl_easy_cleanup(crl);
    }
}
//Зачем я вообще создал C++ модуль, если Rust почти не уступает C++ по скорости? Ну, наверное потому что C++ я лучше знаю... Хотя я никогда не писал на нем тг боты