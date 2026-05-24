/* CORRECTED WryActivity.kt for godot_wry Android integration */

package com.example.godotwry

import android.annotation.SuppressLint
import android.os.Build
import android.os.Bundle
import android.webkit.WebView
import android.view.KeyEvent
import android.view.ViewGroup
import android.widget.FrameLayout
import android.util.Log
import org.godotengine.godot.GodotActivity

open @androidx.annotation.Keep class WryActivity : GodotActivity() {
    private lateinit var mWebView: RustWebView
    private var rootLayout: ViewGroup? = null

    companion object {
        init {
            System.loadLibrary("godot_wry")
        }

        @androidx.annotation.Keep
        fun keepJniClasses() {
            RustWebChromeClient::class.java
            RustWebViewClient::class.java
            RustWebView::class.java
            Ipc::class.java
            Logger::class.java
            PermissionHelper::class.java
        }
    }
    open fun onWebViewCreate(webView: WebView) {
    runOnUiThread {
        val parent = webView.parent as? ViewGroup
        parent?.removeView(webView)

        // Make WebView fully transparent so Godot 3D scene shows through
        webView.background = null
        webView.setBackgroundColor(android.graphics.Color.TRANSPARENT)
        webView.setLayerType(android.view.View.LAYER_TYPE_HARDWARE, null)
        webView.isClickable = true
        webView.isFocusable = true
        webView.isFocusableInTouchMode = true

        val params = FrameLayout.LayoutParams(
            ViewGroup.LayoutParams.MATCH_PARENT,
            ViewGroup.LayoutParams.MATCH_PARENT
        )

        val godotContainerId = resources.getIdentifier(
            "godot_fragment_container", "id", packageName
        )
        val godotContainer = if (godotContainerId != 0) {
            findViewById<ViewGroup>(godotContainerId)
        } else null

        val targetParent = (godotContainer?.parent as? ViewGroup) ?: godotContainer

        if (targetParent != null) {
            webView.z = 100f
            targetParent.addView(webView, params)
        } else {
            val frame = FrameLayout(this)
            frame.addView(webView, params)
            addContentView(frame, params)
        }
    }
}

    fun setWebView(webView: RustWebView) {
        mWebView = webView
        onWebViewCreate(webView)
    }

    val version: String
        @SuppressLint("WebViewApiAvailability", "ObsoleteSdkInt")
        get() {
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                return WebView.getCurrentWebViewPackage()?.versionName ?: ""
            }
            var webViewPackage = "com.google.android.webview"
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
                webViewPackage = "com.android.chrome"
            }
            try {
                @Suppress("DEPRECATION")
                val info = packageManager.getPackageInfo(webViewPackage, 0)
                return info.versionName.toString()
            } catch (ex: Exception) {
                Logger.warn("Unable to get package info for '$webViewPackage'$ex")
            }
            try {
                @Suppress("DEPRECATION")
                val info = packageManager.getPackageInfo("com.android.webview", 0)
                return info.versionName.toString()
            } catch (ex: Exception) {
                Logger.warn("Unable to get package info for 'com.android.webview'$ex")
            }
            return ""
        }

    // ── FIX: Prevent WRY from destroying Godot's layout ───────────────
    // WRY's Android backend natively calls Activity.setContentView(WebView),
    // which completely wipes out Godot's Fragment and SurfaceView.
    // We intercept and ignore it here, letting onWebViewCreate handle placement.
    override fun setContentView(view: android.view.View?) {
        if (view is WebView) {
            Log.w("GodotWry", "Intercepted WRY's setContentView(WebView). Ignoring it.")
            return
        }
        super.setContentView(view)
    }
    
    override fun setContentView(view: android.view.View?, params: android.view.ViewGroup.LayoutParams?) {
        if (view is WebView) {
            Log.w("GodotWry", "Intercepted WRY's setContentView(WebView, params). Ignoring it.")
            return
        }
        super.setContentView(view, params)
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        // Force the main Activity Window to be transparent, allowing Godot's SurfaceView
        // (which punches a hole behind the window) to be visible beneath the transparent WebView.
        window.setBackgroundDrawable(android.graphics.drawable.ColorDrawable(android.graphics.Color.TRANSPARENT))
        window.setFormat(android.graphics.PixelFormat.TRANSPARENT)

        // ── FIX: call nativeInit BEFORE create ───────────────────────────
        // nativeInit calls wry::android_setup() which MUST happen before
        // any WebView is built. create() triggers WRY's internal setup
        // which may try to build a WebView immediately.
        try {
            nativeInit(this)
        } catch (e: UnsatisfiedLinkError) {
            Logger.warn("nativeInit skipped: ${e.message}")
        }

        try {
            create(this)
        } catch (e: UnsatisfiedLinkError) {
            Logger.warn("create skipped: ${e.message}")
        }
    }

    override fun onStart() {
        super.onStart()
        try { start() } catch (e: UnsatisfiedLinkError) {
            Logger.warn("start skipped: ${e.message}")
        }
    }

    override fun onResume() {
        super.onResume()
        try { resume() } catch (e: UnsatisfiedLinkError) {
            Logger.warn("resume skipped: ${e.message}")
        }
    }

    override fun onPause() {
        try { pause() } catch (e: UnsatisfiedLinkError) {
            Logger.warn("pause skipped: ${e.message}")
        }
        super.onPause()
    }

    override fun onStop() {
        try { stop() } catch (e: UnsatisfiedLinkError) {
            Logger.warn("stop skipped: ${e.message}")
        }
        super.onStop()
    }

    override fun onSaveInstanceState(outState: Bundle) {
        super.onSaveInstanceState(outState)
        try { save() } catch (e: UnsatisfiedLinkError) {
            Logger.warn("save skipped: ${e.message}")
        }
    }

    override fun onLowMemory() {
        super.onLowMemory()
        try { memory() } catch (e: UnsatisfiedLinkError) {
            Logger.warn("memory skipped: ${e.message}")
        }
    }

    override fun onWindowFocusChanged(hasFocus: Boolean) {
        super.onWindowFocusChanged(hasFocus)
        try { focus(hasFocus) } catch (e: UnsatisfiedLinkError) {
            Logger.warn("focus skipped: ${e.message}")
        }
    }

    override fun onDestroy() {
        rootLayout = null
        try { destroy() } catch (e: UnsatisfiedLinkError) {
            Logger.warn("destroy skipped: ${e.message}")
        }
        try { onActivityDestroy() } catch (_: UnsatisfiedLinkError) { }
        super.onDestroy()
    }

    override fun onKeyDown(keyCode: Int, event: KeyEvent?): Boolean {
        if (::mWebView.isInitialized && keyCode == KeyEvent.KEYCODE_BACK && mWebView.canGoBack()) {
            mWebView.goBack()
            return true
        }
        return super.onKeyDown(keyCode, event)
    }

    fun getAppClass(name: String): Class<*> {
        return Class.forName(name)
    }

    // ── FIX: wait for Godot's layout to be ready before attaching ────────
    // Godot creates godot_fragment_container during onCreate via super.onCreate().
    // We search for it in the view hierarchy after super has run.
    private fun ensureRootLayout(): ViewGroup? {
        if (rootLayout != null) return rootLayout

        // Walk up from godot_fragment_container to find its parent FrameLayout
        val godotContainerId = resources.getIdentifier(
            "godot_fragment_container", "id", packageName
        )
        if (godotContainerId != 0) {
            val godotContainer = findViewById<FrameLayout>(godotContainerId)
            if (godotContainer != null) {
                // Use the container's parent so WebView overlays on top of Godot
                rootLayout = godotContainer.parent as? ViewGroup
                    ?: godotContainer  // fallback: use the container itself
            }
        }

        // If we still don't have a container, use the window's decor view
        if (rootLayout == null) {
            rootLayout = window.decorView.rootView as? ViewGroup
        }

        return rootLayout
    }

    private external fun nativeInit(activity: WryActivity)
    private external fun create(activity: WryActivity)
    private external fun start()
    private external fun resume()
    private external fun pause()
    private external fun stop()
    private external fun save()
    private external fun destroy()
    private external fun memory()
    private external fun focus(focus: Boolean)
    private external fun onActivityDestroy()
}