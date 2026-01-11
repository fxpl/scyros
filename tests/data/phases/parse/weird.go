func (ps *PrintSettings) GetDoubleWithDefault(key string, def float64) float64 {
	cstr := C.CString(key)
	defer C.free(unsafe.Pointer(cstr))
	c := C.gtk_print_settings_get_double_with_default(ps.native(),
			(*C.gchar)(cstr), C.gdouble(def))
	return float64(c)
}

func polarToCartesian(r, theta float64) (x, y float64) {
	x = r * math.Cos(theta)
	y = r * math.Sin(theta)
	return
}